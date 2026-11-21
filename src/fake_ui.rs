//! The "is this drive real?" window (Function menu → Check if the drive is fake).

use crate::fakecheck::{self, BlockKind, Phase, State, Verdict, BLOCK};
use crate::i18n::t;
use crate::App;
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Share of the free space to test.
const PERCENTS: [u64; 5] = [5, 10, 25, 50, 100];

pub struct FakeUi {
    pub open: bool,
    pub dirs: Vec<(PathBuf, String)>,
    pub dir_idx: usize,
    pub pct_idx: usize,
    /// Quick mode (samples over the whole space) instead of the full test.
    pub quick: bool,
    pub handle: Option<fakecheck::Handle>,
    pub last: Option<State>,
    /// What the drive claims to hold, and what was free when the test began.
    pub claimed: Option<u64>,
    pub free_at_start: u64,
    /// Start a run at the next frame (used by `--try-fake`).
    pub auto_start: bool,
}

impl Default for FakeUi {
    fn default() -> Self {
        FakeUi { open: false, dirs: Vec::new(), dir_idx: 0, pct_idx: 1, quick: true, handle: None, last: None, claimed: None, free_at_start: 0, auto_start: false }
    }
}

/// How many bytes to test for a share of the free space: leaves a margin so the drive is not
/// filled to the last byte, and counts whole blocks.
pub fn amount(free: u64, percent: u64) -> u64 {
    let usable = free.saturating_sub((64u64 << 20).max(free / 100));
    (usable * percent.min(100) / 100) / BLOCK as u64 * BLOCK as u64
}

impl App {
    /// What the disk that holds `dir` says its size is.
    pub(crate) fn claimed_capacity(&self, dir: &Path) -> Option<u64> {
        let mut best: Option<(usize, u64)> = None;
        for d in &self.disks {
            for p in &d.partitions {
                for m in &p.mounts {
                    let md = crate::bench_ui::mount_dir(m);
                    if dir.starts_with(&md) && best.map_or(true, |(l, _)| md.as_os_str().len() > l) {
                        best = Some((md.as_os_str().len(), d.size_bytes));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    pub(crate) fn fake_open(&mut self) {
        self.fake.dirs = self.bench_folders();
        self.fake.dir_idx = self.fake.dir_idx.min(self.fake.dirs.len().saturating_sub(1));
        self.fake.open = true;
    }

    pub(crate) fn fake_window(&mut self, ctx: &egui::Context) {
        if !self.fake.open {
            if let Some(h) = &self.fake.handle {
                h.cancel.store(true, Ordering::Relaxed);
            }
            return;
        }
        let lang = self.settings.language;
        let pal = self.pal();
        let live: Option<State> = self.fake.handle.as_ref().map(|h| h.state.lock().unwrap_or_else(|e| e.into_inner()).clone());
        if let Some(s) = &live {
            if s.running() {
                ctx.request_repaint_after(Duration::from_millis(200));
            } else {
                self.fake.last = live.clone();
                self.fake.handle = None;
            }
        }
        let running = self.fake.handle.is_some();
        let state: Option<State> = live.or_else(|| self.fake.last.clone());

        let mut f = std::mem::take(&mut self.fake);
        let mut start = std::mem::take(&mut f.auto_start);
        let mut stop = false;
        let mut refresh = false;
        let mut mounted: Option<PathBuf> = None;
        let mut open = true;
        let gb = |b: u64| self.decimal(format!("{:.2}", b as f64 / 1e9)) + " GB";
        let free_now: Option<u64> = f.dirs.get(f.dir_idx).and_then(|d| crate::platform::free_bytes(&d.0));

        egui::Window::new(t(lang, "fake_title").trim_end_matches('…'))
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_width(640.0)
            .show(ctx, |ui| {
                ui.label(t(lang, "fake_intro"));
                ui.add_space(6.0);
                ui.add_enabled_ui(!running, |ui| {
                    egui::Grid::new("fake_cfg").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label(t(lang, "bench_folder"));
                        ui.horizontal(|ui| {
                            let shown = f.dirs.get(f.dir_idx).map(|d| crate::bench_ui::dir_label(lang, &d.1)).unwrap_or_else(|| t(lang, "bench_no_dirs").to_string());
                            egui::ComboBox::from_id_salt("fake_dir").width(430.0).selected_text(shown).show_ui(ui, |ui| {
                                for (i, (_, label)) in f.dirs.iter().enumerate() {
                                    ui.selectable_value(&mut f.dir_idx, i, crate::bench_ui::dir_label(lang, label));
                                }
                            });
                            if ui.button("⟳").on_hover_text(t(lang, "bench_refresh")).clicked() {
                                refresh = true;
                            }
                        });
                        ui.end_row();
                        if let Some(p) = crate::bench_ui::mount_row(ui, lang, f.dirs.get(f.dir_idx)) {
                            mounted = Some(p);
                        }
                        ui.label(t(lang, "fake_mode"));
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut f.quick, true, t(lang, "fake_mode_quick"));
                            ui.selectable_value(&mut f.quick, false, t(lang, "fake_mode_full"));
                        });
                        ui.end_row();
                        if !f.quick {
                        ui.label(t(lang, "fake_amount"));
                        ui.horizontal(|ui| {
                            let pct = PERCENTS[f.pct_idx];
                            egui::ComboBox::from_id_salt("fake_pct")
                                .selected_text(if pct == 100 { t(lang, "fake_all").to_string() } else { format!("{pct} %") })
                                .show_ui(ui, |ui| {
                                    for (i, p) in PERCENTS.iter().enumerate() {
                                        let label = if *p == 100 { t(lang, "fake_all").to_string() } else { format!("{p} %") };
                                        ui.selectable_value(&mut f.pct_idx, i, label);
                                    }
                                });
                            if let Some(free) = free_now {
                                ui.label(RichText::new(format!("≈ {} / {} {}", gb(amount(free, pct)), gb(free), t(lang, "col_free").to_lowercase())).color(pal.muted));
                            }
                        });
                        ui.end_row();
                        }
                    });
                });
                if f.quick {
                    ui.label(RichText::new(t(lang, "fake_quick_hint")).color(pal.muted));
                }
                if !f.quick && PERCENTS[f.pct_idx] >= 50 && !running {
                    ui.label(RichText::new(t(lang, "fake_pct_warn")).color(pal.caution));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if running {
                        if ui.button(t(lang, "bench_stop")).clicked() {
                            stop = true;
                        }
                    } else if ui.add_enabled(free_now.is_some() && f.dirs.get(f.dir_idx).is_some_and(|d| !crate::bench_ui::dir_label_locked(&d.1)), egui::Button::new(RichText::new(t(lang, "bench_start")).strong())).clicked() {
                        start = true;
                    }
                });

                if let Some(s) = &state {
                    ui.add_space(6.0);
                    let status = match &s.phase {
                        Phase::Idle => String::new(),
                        Phase::Writing => format!("{} {}/{}", t(lang, "fake_writing"), s.written, s.total),
                        Phase::Verifying => format!("{} {}/{}", t(lang, "fake_verifying"), s.verified, s.written),
                        Phase::Done => String::new(),
                        Phase::Cancelled => t(lang, "bench_cancelled").to_string(),
                        Phase::Failed(e) => format!("{}: {e}", t(lang, "bench_failed")),
                    };
                    if running {
                        ui.add(egui::ProgressBar::new(s.fraction).show_percentage().text(status));
                    } else if !status.is_empty() {
                        let color = if matches!(s.phase, Phase::Failed(_)) { pal.bad } else { pal.muted };
                        ui.label(RichText::new(status).color(color));
                    }

                    // The verdict.
                    if let Some(v) = &s.verdict {
                        let tested = if s.quick { f.free_at_start } else { s.written * BLOCK as u64 };
                        let (title, detail, color) = match v {
                            Verdict::Genuine => (
                                t(lang, "fake_v_genuine").to_string(),
                                format!("{} {}", gb(tested), t(lang, "fake_v_genuine_detail")),
                                pal.good,
                            ),
                            Verdict::Fake { real_bytes } => (
                                t(lang, "fake_v_fake").to_string(),
                                match f.claimed {
                                    Some(c) => format!("{} {} · {} {}", t(lang, "fake_claimed"), gb(c), t(lang, "fake_real"), gb(*real_bytes)),
                                    None => format!("{} {}", t(lang, "fake_real"), gb(*real_bytes)),
                                },
                                pal.bad,
                            ),
                            Verdict::Faulty { bad_blocks } => (
                                t(lang, "fake_v_faulty").to_string(),
                                format!("{bad_blocks} {}", t(lang, "fake_blocks_bad")),
                                pal.caution,
                            ),
                            Verdict::Unreliable => (t(lang, "fake_v_unreliable").to_string(), t(lang, "bench_note_cached").to_string(), pal.unknown),
                        };
                        egui::Frame::new().fill(color).corner_radius(6).inner_margin(10).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(title).size(18.0).strong().color(egui::Color32::WHITE));
                            ui.label(RichText::new(detail).color(egui::Color32::WHITE));
                        });
                        // A genuine result on a part of the space proves only that part.
                        if matches!(v, Verdict::Genuine) && s.quick {
                            ui.add_space(4.0);
                            ui.label(RichText::new(t(lang, "fake_quick_note")).color(pal.caution));
                        } else if matches!(v, Verdict::Genuine) && f.free_at_start > 0 && tested * 10 < f.free_at_start * 9 {
                            ui.add_space(4.0);
                            ui.label(RichText::new(t(lang, "fake_partial_note")).color(pal.caution));
                        }
                    }

                    // Numbers.
                    if s.written > 0 {
                        ui.add_space(6.0);
                        egui::Grid::new("fake_numbers").num_columns(3).spacing([16.0, 3.0]).show(ui, |ui| {
                            ui.label("");
                            ui.label(RichText::new(t(lang, "fake_avg")).strong());
                            ui.label(RichText::new(t(lang, "fake_slowest")).strong());
                            ui.end_row();
                            ui.label(t(lang, "bench_write"));
                            ui.monospace(format!("{} MB/s", self.decimal(format!("{:.1}", s.write_avg))));
                            ui.monospace(format!("{} MB/s", self.decimal(format!("{:.1}", s.write_min))));
                            ui.end_row();
                            if s.verified > 0 {
                                ui.label(t(lang, "bench_read"));
                                ui.monospace(format!("{} MB/s", self.decimal(format!("{:.1}", s.read_avg))));
                                ui.monospace(format!("{} MB/s", self.decimal(format!("{:.1}", s.read_min))));
                                ui.end_row();
                            }
                        });
                        ui.label(RichText::new(format!("{} {} · {} {}", s.verified, t(lang, "fake_blocks_checked"), s.bad_count, t(lang, "fake_blocks_bad"))).color(pal.muted));
                        if s.ran_full {
                            ui.label(RichText::new(t(lang, "fake_ran_full")).color(pal.caution));
                        }
                    }
                    if !s.bad.is_empty() {
                        egui::CollapsingHeader::new(t(lang, "fake_bad_list")).show(ui, |ui| {
                            for b in s.bad.iter().take(12) {
                                let what = match b.kind {
                                    BlockKind::Overwritten(o) => format!("{} {o}", t(lang, "fake_bad_over")),
                                    BlockKind::Empty => t(lang, "fake_bad_empty").to_string(),
                                    BlockKind::Corrupt => t(lang, "fake_bad_corrupt").to_string(),
                                };
                                ui.label(RichText::new(format!("{} {}: {what}", t(lang, "fake_block"), b.index)).monospace());
                            }
                        });
                    }
                }
                ui.add_space(6.0);
                ui.label(RichText::new(t(lang, "fake_note")).small().color(pal.muted));
            });

        if let Some(p) = mounted {
            f.dirs = self.bench_folders();
            f.dir_idx = f.dirs.iter().position(|d| d.0 == p).unwrap_or(0);
        }
        if refresh {
            f.dirs = self.bench_folders();
            f.dir_idx = f.dir_idx.min(f.dirs.len().saturating_sub(1));
        }
        if stop {
            if let Some(h) = &f.handle {
                h.cancel.store(true, Ordering::Relaxed);
            }
        }
        if start {
            if let (Some((dir, _)), Some(free)) = (f.dirs.get(f.dir_idx), free_now) {
                let bytes = amount(free, if f.quick { 100 } else { PERCENTS[f.pct_idx] });
                if bytes >= 8 * BLOCK as u64 {
                    f.claimed = self.claimed_capacity(dir);
                    f.free_at_start = free;
                    f.last = None;
                    f.handle = Some(fakecheck::start(fakecheck::Config { dir: dir.clone(), bytes, quick: f.quick }));
                }
            }
        }
        f.open = open;
        self.fake = f;
    }
}
