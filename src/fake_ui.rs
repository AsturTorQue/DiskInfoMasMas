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
    /// The drive a raw test ran on: after it the person's partition is unmounted, so this drive is chosen.
    pub after_dev: Option<String>,
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
        FakeUi { open: false, dirs: Vec::new(), dir_idx: 0, pct_idx: 1, quick: true, after_dev: None, handle: None, last: None, claimed: None, free_at_start: 0, auto_start: false }
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
        for d in self.pick_disks() {
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

    /// The whole drive that holds `dir` (a mounted folder, or "/dev/sdb1" of a partition that is
    /// not mounted) when the quick test may work on it directly (Linux): its device, its size and
    /// the partitions to unmount first. Never for a drive that holds the system or the home folder.
    pub(crate) fn raw_target(&self, dir: &Path) -> Option<(String, u64, Vec<String>)> {
        if !cfg!(target_os = "linux") {
            return None;
        }
        // A folder chosen by hand is tested with files in it, like H2testw does.
        if self.custom_dirs.iter().any(|c| c == dir) {
            return None;
        }
        let mut best: Option<(usize, &crate::disks::Disk)> = None;
        for d in self.pick_disks() {
            if Path::new(&format!("/dev/{}", d.dev)) == dir && best.is_none() {
                best = Some((0, d));
            }
            for p in &d.partitions {
                if Path::new(&format!("/dev/{}", p.name)) == dir && best.is_none() {
                    best = Some((0, d));
                }
                for m in &p.mounts {
                    let md = crate::bench_ui::mount_dir(m);
                    if dir.starts_with(&md) && best.map_or(true, |(l, _)| md.as_os_str().len() >= l) {
                        best = Some((md.as_os_str().len(), d));
                    }
                }
            }
        }
        let (_, d) = best?;
        if crate::drive_picker::is_system_disk(d) || d.dev.is_empty() || d.size_bytes == 0 || !d.dev.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        let to_unmount = d.partitions.iter().filter(|p| !p.mounts.is_empty()).map(|p| format!("/dev/{}", p.name)).collect();
        Some((format!("/dev/{}", d.dev), d.size_bytes, to_unmount))
    }

    pub(crate) fn fake_open(&mut self) {
        self.fake.dirs = self.bench_folders();
        self.fake.dir_idx = self.fake.dir_idx.min(self.fake.dirs.len().saturating_sub(1));
        self.fake.open = true;
        self.grow_to = Some((1260.0, 820.0));
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
                // The drives changed (a raw test unmounts them): read them again.
                self.storage_force = true;
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
        // The quick mode works on the whole drive directly. Where that is not possible (Windows and
        // macOS for now, system disks, folders chosen by hand) files would have to fill the gaps,
        // which is slower than the full test: then only the full test is offered.
        let raw_possible = f.dirs.get(f.dir_idx).and_then(|d| self.raw_target(&d.0));
        if raw_possible.is_none() && f.quick && !running {
            f.quick = false;
        }
        let raw = if f.quick { raw_possible.clone() } else { None };
        let free_now: Option<u64> = f.dirs.get(f.dir_idx).and_then(|d| crate::platform::free_bytes(&d.0));

        egui::Window::new(t(lang, "fake_title").trim_end_matches('…'))
            .id(egui::Id::new("fake_window_v4"))
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size([(ctx.screen_rect().width() * 0.9).min(1240.0).max((ctx.screen_rect().width() * 0.98).min(860.0)), ctx.screen_rect().height() * 0.9])
            .min_width((ctx.screen_rect().width() * 0.95).min(780.0))
            .max_width(ctx.screen_rect().width() * 0.98)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.label(t(lang, "fake_intro"));
                ui.add_space(6.0);
                // Drives on the left; mode, results and the rest on the right.
                // Drives on the left (about 40 %), the rest on the right.
                let total_w = ui.available_width();
                let left_w = (total_w * 0.4).clamp(300.0, 520.0).min((total_w - 380.0).max(260.0));
                ui.horizontal_top(|ui| {
                let avail_h = ui.available_height();
                ui.allocate_ui_with_layout(egui::vec2(left_w, avail_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                // What does not fit a column is cut, never drawn over the other one.
                let mut clip = ui.clip_rect();
                clip.max.x = clip.max.x.min(ui.max_rect().right());
                ui.set_clip_rect(clip);
                ui.add_enabled_ui(!running, |ui| {
                    let mode = if f.quick { crate::drive_picker::PickMode::FakeQuick } else { crate::drive_picker::PickMode::FakeFull };
                    let (r, m) = self.drive_picker(ui, &f.dirs, &mut f.dir_idx, mode, 170.0);
                    refresh |= r;
                    if m.is_some() {
                        mounted = m;
                    }
                });
                });
                ui.add_space(8.0);
                ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), avail_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                let mut clip = ui.clip_rect();
                clip.max.x = clip.max.x.min(ui.max_rect().right());
                ui.set_clip_rect(clip);
                ui.vertical(|ui| {
                ui.add_enabled_ui(!running, |ui| {
                    egui::Grid::new("fake_cfg").num_columns(2).min_col_width(120.0).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label(t(lang, "fake_mode"));
                        ui.vertical(|ui| {
                            ui.add_enabled_ui(raw_possible.is_some(), |ui| ui.selectable_value(&mut f.quick, true, t(lang, "fake_mode_quick")))
                                .response
                                .on_disabled_hover_text(t(lang, "fake_quick_unavailable"));
                            ui.selectable_value(&mut f.quick, false, t(lang, "fake_mode_full"));
                        });
                        ui.end_row();
                        // Always there (greyed out in quick mode), so choosing a mode moves nothing.
                        ui.add_enabled(!f.quick, egui::Label::new(t(lang, "fake_amount")));
                        ui.add_enabled_ui(!f.quick, |ui| ui.vertical(|ui| {
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
                        }));
                        ui.end_row();
                    });
                });
                // A text area of the same height in both modes.
                ui.allocate_ui_with_layout(egui::Vec2::new(ui.available_width(), 84.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_min_height(84.0);
                    if f.quick {
                        let key = if raw.is_some() { "fake_quick_raw_hint" } else { "fake_quick_hint" };
                        ui.label(RichText::new(t(lang, key)).color(pal.muted));
                    } else if PERCENTS[f.pct_idx] >= 50 && !running {
                        ui.label(RichText::new(t(lang, "fake_pct_warn")).color(pal.caution));
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if running {
                        if ui.add(egui::Button::new(RichText::new(t(lang, "bench_stop")).size(18.0).strong()).min_size(egui::Vec2::new(200.0, 44.0))).clicked() {
                            stop = true;
                        }
                    } else if ui.add_enabled(raw.is_some() || (free_now.is_some() && f.dirs.get(f.dir_idx).is_some_and(|d| !crate::bench_ui::dir_label_locked(&d.1))), egui::Button::new(RichText::new(t(lang, "bench_start")).size(18.0).strong()).min_size(egui::Vec2::new(200.0, 44.0))).clicked() {
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
                        ui.add_space(4.0);
                    } else if matches!(s.phase, Phase::Failed(_)) {
                        egui::Frame::new().fill(pal.bad).corner_radius(6).inner_margin(10).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(status).strong().color(egui::Color32::WHITE));
                        });
                    } else if !status.is_empty() {
                        ui.label(RichText::new(status).color(pal.muted));
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

                    // Speed now and its graph.
                    crate::drive_picker::speed_graph(ui, &pal, s, t(lang, "fake_speed_now"), (t(lang, "bench_write"), t(lang, "bench_read")), |x| self.decimal(x));
                    // The block map of the test.
                    if s.written > 0 || running {
                        ui.add_space(6.0);
                        let span = if s.quick { f.free_at_start } else { s.total * BLOCK as u64 };
                        crate::drive_picker::progress_blocks(ui, &pal, s, span);
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
                });
                });
            });

        if let Some(p) = mounted {
            // Read the drives again at once, so the new mount is there to choose.
            self.storage = crate::disks::storage_snapshot();
            f.dirs = self.bench_folders();
            f.dir_idx = f.dirs.iter().position(|d| d.0 == p).unwrap_or(0);
        }
        if refresh {
            self.storage_force = true;
            f.dirs = self.bench_folders();
            f.dir_idx = f.dir_idx.min(f.dirs.len().saturating_sub(1));
        }
        if stop {
            if let Some(h) = &f.handle {
                h.cancel.store(true, Ordering::Relaxed);
            }
        }
        #[cfg(target_os = "linux")]
        if start && f.quick {
            if let Some((dev, size, unmount)) = raw {
                start = false;
                f.claimed = Some(size);
                f.free_at_start = size;
                f.last = None;
                // The drive must not be mounted while its spots are rewritten.
                let failed = unmount.iter().find_map(|p| {
                    let out = std::process::Command::new("udisksctl").args(["unmount", "-b", p]).output();
                    match out {
                        Ok(o) if o.status.success() => None,
                        // Already unmounted (by an earlier test or from outside): nothing to do.
                        Ok(o) if String::from_utf8_lossy(&o.stderr).contains("not mounted") => None,
                        Ok(o) => Some(String::from_utf8_lossy(&o.stderr).trim().to_string()),
                        Err(e) => Some(e.to_string()),
                    }
                });
                match failed {
                    None => {
                        f.after_dev = dev.strip_prefix("/dev/").map(str::to_string);
                        f.handle = Some(crate::rawcheck::start(dev));
                    }
                    Some(e) => {
                        let mut s = State::new(&fakecheck::Config { dir: PathBuf::new(), bytes: 0, quick: true });
                        s.phase = Phase::Failed(e);
                        f.last = Some(s);
                    }
                }
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
