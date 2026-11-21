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
    /// The folder the size was last adjusted for (so it is fitted once per folder, not every frame).
    pub size_dir: Option<PathBuf>,
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
            size_dir: None,
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
pub(crate) fn mount_dir(m: &str) -> PathBuf {
    if m.len() == 2 && m.ends_with(':') {
        PathBuf::from(format!("{m}\\"))
    } else {
        PathBuf::from(m)
    }
}

/// Every mounted file system of a real disk that the system lists (not only the disks read for
/// S.M.A.R.T.): (folder, file system).
fn system_mounts() -> Vec<(PathBuf, String)> {
    let mut v = Vec::new();
    #[cfg(target_os = "linux")]
    if let Ok(text) = std::fs::read_to_string("/proc/mounts") {
        const REAL: [&str; 14] = ["ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "vfat", "exfat", "ntfs", "ntfs3", "fuseblk", "jfs", "reiserfs", "udf"];
        for line in text.lines() {
            let mut f = line.split_whitespace();
            let (Some(dev), Some(mp), Some(fs)) = (f.next(), f.next(), f.next()) else { continue };
            if !dev.starts_with('/') || !REAL.contains(&fs) || mp.starts_with("/snap") || mp.contains("/flatpak/") || mp.starts_with("/var/lib/") {
                continue;
            }
            // Bind mounts of immutable systems (Bazzite, Silverblue...) are not drives to pick.
            if ["/etc", "/sysroot", "/usr", "/var/opt", "/var/log", "/var/tmp", "/opt"].iter().any(|p| mp == *p || mp.starts_with(&format!("{p}/"))) || mp == "/var" {
                continue;
            }
            // Spaces are written as \040 in this file.
            v.push((PathBuf::from(mp.replace("\\040", " ")), fs.to_string()));
        }
    }
    #[cfg(windows)]
    for c in b'A'..=b'Z' {
        let p = PathBuf::from(format!("{}:\\", c as char));
        if p.is_dir() {
            v.push((p, String::new()));
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        for e in rd.flatten() {
            v.push((e.path(), String::new()));
        }
    }
    v
}

/// The person's home folder when it is inside `mount` and can be written to.
fn home_inside(mount: &Path) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)?;
    let (home, mount) = (home.canonicalize().ok()?, mount.canonicalize().ok()?);
    (home.starts_with(&mount) && writable(&home)).then_some(home)
}

/// True for a folder entry that cannot be tested in as it is: no write access, or a partition
/// that is not mounted yet (see `bench_folders`).
pub(crate) fn dir_label_locked(label: &str) -> bool {
    label.ends_with(LOCKED) || label.ends_with(UNMOUNTED) || label.ends_with(WHOLE)
}

const UNMOUNTED: &str = " [u]";
/// A whole drive (only the quick fake-drive test works on these).
pub(crate) const WHOLE: &str = " [w]";

/// Mounts a partition the way a file manager does (no administrator needed on most desktops);
/// returns the folder it was mounted at.
pub(crate) fn mount_device(dev: &Path) -> Result<PathBuf, String> {
    let out = std::process::Command::new("udisksctl").args(["mount", "-b"]).arg(dev).output().map_err(|e| format!("udisksctl: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    // "Mounted /dev/sdb1 at /run/media/ana/USB." or "... already mounted at `/run/media/ana/USB'."
    let at = text.rsplit(" at ").next().unwrap_or("").trim().trim_start_matches('`').trim_end_matches(['.', '\'', '\n', ' ']);
    if (out.status.success() || text.contains("already mounted")) && !at.is_empty() && Path::new(at).is_dir() {
        Ok(PathBuf::from(at))
    } else {
        Err(text.trim().to_string())
    }
}

const LOCKED: &str = " [x]";

/// The label as shown: the marker of a folder without write access becomes words.
pub(crate) fn dir_label(lang: crate::i18n::Lang, label: &str) -> String {
    if let Some(l) = label.strip_suffix(LOCKED) {
        format!("{l} - {}", t(lang, "bench_locked"))
    } else if let Some(l) = label.strip_suffix(WHOLE) {
        l.to_string()
    } else if let Some(l) = label.strip_suffix(UNMOUNTED) {
        format!("{l} - {}", t(lang, "bench_unmounted"))
    } else {
        label.to_string()
    }
}

impl App {
    /// Folders of the disks on screen where a test file can be made, and the person's own folders.
    pub(crate) fn bench_folders(&self) -> Vec<(PathBuf, String)> {
        let mut out: Vec<(PathBuf, String)> = Vec::new();
        let add = |dir: PathBuf, label: String, out: &mut Vec<(PathBuf, String)>| {
            if !dir.is_dir() {
                return;
            }
            if writable(&dir) {
                if !out.iter().any(|(p, _)| *p == dir) {
                    out.push((dir, label));
                }
            } else if let Some(home) = home_inside(&dir) {
                // The root of the mount belongs to the system (like /var/home), but the person's
                // own folder on it can be written to.
                if !out.iter().any(|(p, _)| *p == home) {
                    out.push((home.clone(), format!("{label} > {}", home.display())));
                }
            } else if !out.iter().any(|(p, _)| *p == dir) {
                // A mounted drive the person cannot write to is still listed, marked.
                out.push((dir, format!("{label}{LOCKED}")));
            }
        };
        for d in self.pick_disks() {
            for p in &d.partitions {
                for m in &p.mounts {
                    let free = p.free.map(|f| format!(" · {} {}", self.bytes_text(f), t(self.settings.language, "col_free").to_lowercase())).unwrap_or_default();
                    add(mount_dir(m), format!("{m} — {}{free}", d.model), &mut out);
                }
            }
        }
        for (mp, fs) in system_mounts() {
            let free = crate::platform::free_bytes(&mp).map(|f| format!(" · {} {}", self.bytes_text(f), t(self.settings.language, "col_free").to_lowercase())).unwrap_or_default();
            let fs = if fs.is_empty() { String::new() } else { format!(" ({fs})") };
            add(mp.clone(), format!("{}{fs}{free}", mp.display()), &mut out);
        }
        if let Some(docs) = crate::platform::documents_dir() {
            add(docs.clone(), docs.display().to_string(), &mut out);
        }
        // Folders the person chose by hand.
        for c in &self.custom_dirs {
            add(c.clone(), c.display().to_string(), &mut out);
        }
        // Partitions that are not mounted (a USB stick just plugged in): offered with a Mount button.
        #[cfg(target_os = "linux")]
        for d in self.pick_disks() {
            for p in &d.partitions {
                let fs = p.fs.as_str();
                let skip = fs.is_empty() || ["swap", "LVM", "raid", "crypto", "zfs_member"].iter().any(|k| fs.contains(k)) || p.kind.contains("EFI System");
                if p.mounts.is_empty() && !skip && !p.name.is_empty() {
                    let dev = PathBuf::from(format!("/dev/{}", p.name));
                    let size = self.bytes_text(p.size);
                    out.push((dev.clone(), format!("{} — {} ({fs}, {size}){UNMOUNTED}", dev.display(), d.model)));
                }
            }
        }
        // Every whole drive, for the quick fake-drive test.
        #[cfg(target_os = "linux")]
        for d in self.pick_disks() {
            if !d.dev.is_empty() && !d.partitions.is_empty() {
                out.push((PathBuf::from(format!("/dev/{}", d.dev)), format!("/dev/{} — {}{WHOLE}", d.dev, d.model)));
            }
        }
        out
    }

    pub(crate) fn bench_open(&mut self) {
        self.bench.dirs = self.bench_folders();
        self.bench.dir_idx = self.bench.dir_idx.min(self.bench.dirs.len().saturating_sub(1));
        self.bench.open = true;
        self.grow_to = Some((1260.0, 820.0));
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
        let mut mounted: Option<PathBuf> = None;
        let mut copy: Option<String> = None;
        let mut open = true;
        let (read_w, write_w) = (t(lang, "bench_read"), t(lang, "bench_write"));
        // The test file must fit in the free space: sizes that do not are greyed out, and a size
        // that is too big for the chosen folder is brought down to the largest that fits.
        let free_now: Option<u64> = b.dirs.get(b.dir_idx).and_then(|d| crate::platform::free_bytes(&d.0));
        let fits = |size: u64| free_now.map_or(true, |f| size + (32 << 20) <= f);
        if !running && b.size_dir.as_ref() != b.dirs.get(b.dir_idx).map(|d| &d.0) {
            b.size_dir = b.dirs.get(b.dir_idx).map(|d| d.0.clone());
            if !fits(SIZES[b.size_idx].0) {
                b.size_idx = (0..SIZES.len()).rev().find(|i| fits(SIZES[*i].0 )).unwrap_or(0).min(b.size_idx);
            }
        }
        let size_ok = fits(SIZES[b.size_idx].0);

        egui::Window::new(t(lang, "bench_title").trim_end_matches('…'))
            .id(egui::Id::new("bench_window_v4"))
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size([(ctx.screen_rect().width() * 0.9).min(1240.0).max((ctx.screen_rect().width() * 0.98).min(860.0)), ctx.screen_rect().height() * 0.9])
            .min_width((ctx.screen_rect().width() * 0.95).min(780.0))
            .max_width(ctx.screen_rect().width() * 0.98)
            .vscroll(true)
            .show(ctx, |ui| {
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
                    let (r, m) = self.drive_picker(ui, &b.dirs, &mut b.dir_idx, crate::drive_picker::PickMode::Speed, 80.0);
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
                // A table wider than the column scrolls sideways instead of being cut.
                egui::ScrollArea::horizontal().id_salt("bench_right").auto_shrink([false, true]).show(ui, |ui| {
                ui.add_enabled_ui(!running, |ui| {
                    egui::Grid::new("bench_cfg").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label(t(lang, "bench_profile"));
                        egui::ComboBox::from_id_salt("bench_profile")
                            .selected_text(t(lang, if b.nvme_profile { "bench_prof_nvme" } else { "bench_prof_default" }))
                            .width(250.0)
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
                                for (i, (size, label)) in SIZES.iter().enumerate() {
                                    ui.add_enabled_ui(fits(*size), |ui| ui.selectable_value(&mut b.size_idx, i, *label));
                                }
                            });
                        });
                        ui.end_row();
                    });
                    if !size_ok {
                        let free_text = free_now.map(|f| self.bytes_text(f)).unwrap_or_default();
                        ui.label(RichText::new(t(lang, "bench_size_warn").replace("{free}", &free_text)).color(pal.caution));
                    }
                    // The numbers go on their own line that wraps, so nothing is drawn over anything.
                    ui.horizontal_wrapped(|ui| {
                        ui.label(t(lang, "bench_runs"));
                        ui.add(egui::DragValue::new(&mut b.runs).range(1..=9));
                        ui.add_space(8.0);
                        ui.label(t(lang, "bench_secs"));
                        ui.add(egui::DragValue::new(&mut b.secs).range(1..=60).suffix(" s"));
                        ui.add_space(8.0);
                        ui.label(t(lang, "bench_interval"));
                        ui.add(egui::DragValue::new(&mut b.interval).range(0..=30).suffix(" s"));
                    });
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if running {
                        if ui.add(egui::Button::new(RichText::new(t(lang, "bench_stop")).size(18.0).strong()).min_size(egui::Vec2::new(200.0, 44.0))).clicked() {
                            stop = true;
                        }
                    } else if ui.add_enabled(size_ok && b.dirs.get(b.dir_idx).is_some_and(|d| !dir_label_locked(&d.1)), egui::Button::new(RichText::new(t(lang, "bench_start")).size(18.0).strong()).min_size(egui::Vec2::new(200.0, 44.0))).clicked() {
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
                });
                });
            });

        if let Some(p) = mounted {
            // Read the drives again at once, so the new mount is there to choose.
            self.storage = crate::disks::storage_snapshot();
            b.dirs = self.bench_folders();
            b.dir_idx = b.dirs.iter().position(|d| d.0 == p).unwrap_or(0);
        }
        if refresh {
            self.storage_force = true;
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

