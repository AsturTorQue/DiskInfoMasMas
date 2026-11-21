//! The speed-test window (Function menu → Disk speed test).

use crate::bench::{self, Pattern, Phase, State};
use crate::i18n::t;
use crate::App;
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

const SIZES: [(u64, &str); 11] = [
    (16 << 20, "16 MiB"),
    (64 << 20, "64 MiB"),
    (256 << 20, "256 MiB"),
    (512 << 20, "512 MiB"),
    (1 << 30, "1 GiB"),
    (2 << 30, "2 GiB"),
    (4 << 30, "4 GiB"),
    (8 << 30, "8 GiB"),
    (16 << 30, "16 GiB"),
    (32 << 30, "32 GiB"),
    (64 << 30, "64 GiB"),
];

/// What the window remembers between frames.
pub struct BenchUi {
    pub open: bool,
    /// Folders to test in: (path, text shown).
    pub dirs: Vec<(PathBuf, String)>,
    pub dir_idx: usize,
    pub size_idx: usize,
    pub runs: u32,
    pub secs: u32,
    pub interval: u32,
    pub pattern: Pattern,
    pub nvme_profile: bool,
    pub handle: Option<bench::Handle>,
    /// The last finished (or cancelled) run, shown while nothing runs.
    pub last: Option<State>,
    /// Start a run at the next frame (used by `--try-bench`).
    pub auto_start: bool,
}

impl Default for BenchUi {
    fn default() -> Self {
        BenchUi {
            open: false,
            dirs: Vec::new(),
            dir_idx: 0,
            size_idx: 4, // 1 GiB, like CrystalDiskMark
            runs: 1,
            secs: 5,
            interval: 1,
            pattern: Pattern::Random,
            nvme_profile: true,
            handle: None,
            last: None,
            auto_start: false,
        }
    }
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".diskinfo-write-test-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// "C:" → "C:\" (a bare drive letter means the current folder of that drive).
fn mount_dir(m: &str) -> PathBuf {
    if m.len() == 2 && m.ends_with(':') {
        PathBuf::from(format!("{m}\\"))
    } else {
        PathBuf::from(m)
    }
}

impl App {
    /// Folders of the disks on screen where a test file can be made, and the person's own folders.
    fn bench_folders(&self) -> Vec<(PathBuf, String)> {
        let mut out: Vec<(PathBuf, String)> = Vec::new();
        let add = |dir: PathBuf, label: String, out: &mut Vec<(PathBuf, String)>| {
            if !out.iter().any(|(p, _)| *p == dir) && dir.is_dir() && writable(&dir) {
                out.push((dir, label));
            }
        };
        for d in &self.disks {
            for p in &d.partitions {
                for m in &p.mounts {
                    let free = p.free.map(|f| format!(" · {} {}", self.bytes_text(f), t(self.settings.language, "col_free").to_lowercase())).unwrap_or_default();
                    add(mount_dir(m), format!("{m} — {}{free}", d.model), &mut out);
                }
            }
        }
        if let Some(docs) = crate::platform::documents_dir() {
            add(docs.clone(), docs.display().to_string(), &mut out);
        }
        out
    }

    pub(crate) fn bench_open(&mut self) {
        self.bench.dirs = self.bench_folders();
        self.bench.dir_idx = self.bench.dir_idx.min(self.bench.dirs.len().saturating_sub(1));
        self.bench.open = true;
    }

    pub(crate) fn bench_window(&mut self, ctx: &egui::Context) {
        if !self.bench.open {
            // Closing the window stops a test that is still running.
            if let Some(h) = &self.bench.handle {
                h.cancel.store(true, Ordering::Relaxed);
            }
            return;
        }
        let lang = self.settings.language;
        let pal = self.pal();
        // A run that has ended moves to `last`.
        let live: Option<State> = self.bench.handle.as_ref().map(|h| h.state.lock().unwrap_or_else(|e| e.into_inner()).clone());
        if let Some(s) = &live {
            if s.running() {
                ctx.request_repaint_after(Duration::from_millis(150));
            } else {
                self.bench.last = live.clone();
                self.bench.handle = None;
            }
        }
        let running = self.bench.handle.is_some();
        let state: Option<State> = live.or_else(|| self.bench.last.clone());

        let mut b = std::mem::take(&mut self.bench);
        let mut start = std::mem::take(&mut b.auto_start);
        let mut stop = false;
        let mut refresh = false;
        let mut copy: Option<String> = None;
        let mut open = true;
        let (read_w, write_w) = (t(lang, "bench_read"), t(lang, "bench_write"));

        egui::Window::new(t(lang, "bench_title").trim_end_matches('…'))
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_width(700.0)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!running, |ui| {
                    egui::Grid::new("bench_cfg").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label(t(lang, "bench_folder"));
                        ui.horizontal(|ui| {
                            let shown = b.dirs.get(b.dir_idx).map(|d| d.1.clone()).unwrap_or_else(|| t(lang, "bench_no_dirs").to_string());
                            egui::ComboBox::from_id_salt("bench_dir").width(430.0).selected_text(shown).show_ui(ui, |ui| {
                                for (i, (_, label)) in b.dirs.iter().enumerate() {
                                    ui.selectable_value(&mut b.dir_idx, i, label);
                                }
                            });
                            if ui.button("⟳").on_hover_text(t(lang, "bench_refresh")).clicked() {
                                refresh = true;
                            }
                        });
                        ui.end_row();

                        ui.label(t(lang, "bench_profile"));
                        egui::ComboBox::from_id_salt("bench_profile")
                            .selected_text(t(lang, if b.nvme_profile { "bench_prof_nvme" } else { "bench_prof_default" }))
                            .width(430.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut b.nvme_profile, true, t(lang, "bench_prof_nvme"));
                                ui.selectable_value(&mut b.nvme_profile, false, t(lang, "bench_prof_default"));
                            });
                        ui.end_row();

                        ui.label(t(lang, "bench_data")).on_hover_text(t(lang, "bench_h_data"));
                        egui::ComboBox::from_id_salt("bench_data")
                            .selected_text(match b.pattern {
                                Pattern::Random => t(lang, "bench_data_random"),
                                Pattern::Zeros => t(lang, "bench_data_zero"),
                                Pattern::Ones => t(lang, "bench_data_ones"),
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut b.pattern, Pattern::Random, t(lang, "bench_data_random"));
                                ui.selectable_value(&mut b.pattern, Pattern::Zeros, t(lang, "bench_data_zero"));
                                ui.selectable_value(&mut b.pattern, Pattern::Ones, t(lang, "bench_data_ones"));
                            })
                            .response
                            .on_hover_text(t(lang, "bench_h_data"));
                        ui.end_row();

                        ui.label(t(lang, "bench_size"));
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("bench_size").selected_text(SIZES[b.size_idx].1).show_ui(ui, |ui| {
                                for (i, (_, label)) in SIZES.iter().enumerate() {
                                    ui.selectable_value(&mut b.size_idx, i, *label);
                                }
                            });
                            ui.label(t(lang, "bench_runs"));
                            ui.add(egui::DragValue::new(&mut b.runs).range(1..=9));
                            ui.label(t(lang, "bench_secs"));
                            ui.add(egui::DragValue::new(&mut b.secs).range(1..=60).suffix(" s"));
                            ui.label(t(lang, "bench_interval"));
                            ui.add(egui::DragValue::new(&mut b.interval).range(0..=30).suffix(" s"));
                        });
                        ui.end_row();
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if running {
                        if ui.button(t(lang, "bench_stop")).clicked() {
                            stop = true;
                        }
                    } else if ui.add_enabled(!b.dirs.is_empty(), egui::Button::new(RichText::new(t(lang, "bench_start")).strong())).clicked() {
                        start = true;
                    }
                    if let Some(s) = &state {
                        if ui.add_enabled(s.results.iter().any(|r| r.read.is_some() || r.write.is_some()), egui::Button::new(t(lang, "bench_copy"))).clicked() {
                            copy = Some(bench::format_table(&s.results, read_w, write_w));
                        }
                    }
                });

                if let Some(s) = &state {
                    ui.add_space(4.0);
                    // Status line and progress.
                    let status = match &s.phase {
                        Phase::Idle => String::new(),
                        Phase::Preparing => t(lang, "bench_preparing").to_string() + "…",
                        Phase::Testing { index, write, run } => format!(
                            "{} {}  ({}/{})",
                            s.results[*index].spec.name,
                            if *write { write_w } else { read_w },
                            run + 1,
                            b.runs.max(1)
                        ),
                        Phase::Done => t(lang, "bench_done").to_string(),
                        Phase::Cancelled => t(lang, "bench_cancelled").to_string(),
                        Phase::Failed(e) => format!("{}: {e}", t(lang, "bench_failed")),
                    };
                    if running {
                        ui.add(egui::ProgressBar::new(s.fraction).show_percentage().text(status));
                    } else {
                        let color = if matches!(s.phase, Phase::Failed(_)) { pal.bad } else { pal.muted };
                        ui.label(RichText::new(status).color(color));
                    }
                    ui.add_space(4.0);

                    // Results.
                    egui::Grid::new("bench_results").striped(true).num_columns(7).spacing([14.0, 4.0]).min_row_height(24.0).show(ui, |ui| {
                        ui.label("");
                        ui.label(RichText::new(format!("{read_w} MB/s")).strong());
                        ui.label(RichText::new("IOPS").strong());
                        ui.label(RichText::new("µs").strong()).on_hover_text(t(lang, "bench_h_latency"));
                        ui.label(RichText::new(format!("{write_w} MB/s")).strong());
                        ui.label(RichText::new("IOPS").strong());
                        ui.label(RichText::new("µs").strong()).on_hover_text(t(lang, "bench_h_latency"));
                        ui.end_row();
                        for r in &s.results {
                            ui.label(RichText::new(r.spec.name).strong().monospace());
                            for p in [&r.read, &r.write] {
                                match p {
                                    Some(p) => {
                                        ui.label(RichText::new(self.decimal(format!("{:.2}", p.mbps))).size(16.0).strong().color(pal.accent));
                                        ui.label(RichText::new(format!("{:.0}", p.iops)).monospace());
                                        ui.label(RichText::new(self.decimal(format!("{:.1}", p.latency_us))).monospace());
                                    }
                                    None => {
                                        for _ in 0..3 {
                                            ui.label(RichText::new("—").color(pal.muted));
                                        }
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });
                    if !s.direct {
                        ui.add_space(4.0);
                        ui.label(RichText::new(t(lang, "bench_note_cached")).color(pal.bad));
                    }
                }
                ui.add_space(6.0);
                ui.label(RichText::new(t(lang, "bench_note_wear")).small().color(pal.muted));
                ui.label(RichText::new(t(lang, "bench_note_threads")).small().color(pal.muted));
            });

        if refresh {
            b.dirs = self.bench_folders();
            b.dir_idx = b.dir_idx.min(b.dirs.len().saturating_sub(1));
        }
        if stop {
            if let Some(h) = &b.handle {
                h.cancel.store(true, Ordering::Relaxed);
            }
        }
        if start {
            if let Some((dir, _)) = b.dirs.get(b.dir_idx) {
                let cfg = bench::Config {
                    dir: dir.clone(),
                    size: SIZES[b.size_idx].0,
                    runs: b.runs.clamp(1, 9),
                    secs: b.secs.clamp(1, 60) as f64,
                    interval: b.interval.min(30) as f64,
                    pattern: b.pattern,
                    specs: if b.nvme_profile { bench::PROFILE_NVME.to_vec() } else { bench::PROFILE_DEFAULT.to_vec() },
                };
                b.last = None;
                b.handle = Some(bench::start(cfg));
            }
        }
        b.open = open;
        self.bench = b;
        if let Some(text) = copy {
            ctx.copy_text(text);
            self.toast_key("report_copied");
        }
    }
}
