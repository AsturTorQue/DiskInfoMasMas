//! The "which drive?" panel shared by the speed test and the fake-drive test: every disk with a
//! block bar of its partitions, the partitions listed under it, what each one allows, and a box
//! saying exactly what the test will do (and that nothing is erased).

use crate::bench_ui::{dir_label_locked, mount_dir};
use crate::disks::Disk;
use crate::i18n::t;
use crate::parts::Partition;
use crate::App;
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
pub enum PickMode {
    Speed,
    FakeQuick,
    FakeFull,
}

#[derive(Clone, Copy, PartialEq)]
enum Access {
    /// Files can be written.
    Ready,
    /// Mounted, but the person cannot write there.
    Locked,
    /// Not mounted yet.
    Unmounted,
    /// Nothing to test (no file system).
    None,
}

/// Disks that hold the running system or the person's home folder.
pub(crate) fn is_system_disk(d: &Disk) -> bool {
    const SYSTEM: [&str; 8] = ["/boot", "/usr", "/etc", "/sysroot", "/home", "/var/home", "/var/lib", "/opt"];
    let win = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    d.partitions.iter().flat_map(|p| p.mounts.iter()).any(|m| {
        m == "/" || m == "/var" || m.eq_ignore_ascii_case(&win) || SYSTEM.iter().any(|s| m == s || m.starts_with(&format!("{s}/")))
    })
}

fn removable(d: &Disk) -> bool {
    d.details.iter().any(|(k, v)| *k == "d_removable" && v == "yes")
}

/// The entry of `dirs` that belongs to a partition: its mount (or the person's folder inside
/// it), or its device when it is not mounted.
fn entry_of(p: &Partition, dirs: &[(PathBuf, String)]) -> Option<usize> {
    if p.mounts.is_empty() {
        let dev = PathBuf::from(format!("/dev/{}", p.name));
        return dirs.iter().position(|d| d.0 == dev);
    }
    for m in &p.mounts {
        let md = mount_dir(m);
        if let Some(i) = dirs.iter().position(|d| d.0 == md) {
            return Some(i);
        }
    }
    for m in &p.mounts {
        let md = mount_dir(m);
        if let Some(i) = dirs.iter().position(|d| d.0.starts_with(&md) && d.0 != md && !d.1.ends_with(" [u]")) {
            return Some(i);
        }
    }
    None
}

fn access_of(entry: Option<usize>, dirs: &[(PathBuf, String)]) -> Access {
    match entry {
        None => Access::None,
        Some(i) if dirs[i].1.ends_with(" [u]") => Access::Unmounted,
        Some(i) if dir_label_locked(&dirs[i].1) => Access::Locked,
        Some(_) => Access::Ready,
    }
}

impl App {
    fn fs_color(&self, fs: &str) -> Color32 {
        let f = fs.to_lowercase();
        let pal = self.pal();
        if f.contains("ntfs") || f.contains("exfat") || f.contains("fat") {
            pal.accent
        } else if f.contains("btrfs") || f.contains("ext") || f.contains("xfs") || f.contains("apfs") {
            pal.good
        } else {
            pal.unknown
        }
    }

    /// Draws the panel. Returns true when the refresh button was pressed.
    pub(crate) fn drive_picker(&self, ui: &mut egui::Ui, dirs: &[(PathBuf, String)], idx: &mut usize, mode: PickMode, reserve: f32) -> bool {
        let lang = self.settings.language;
        let pal = self.pal();
        let mut refresh = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(t(lang, "pick_title")).strong());
            if ui.button("⟳").on_hover_text(t(lang, "bench_refresh")).clicked() {
                refresh = true;
            }
        });
        let mut matched = vec![false; dirs.len()];
        egui::ScrollArea::vertical().max_height((ui.ctx().screen_rect().height() * 0.88 - reserve).max(200.0)).auto_shrink([false, true]).show(ui, |ui| {
            for d in &self.disks {
                if d.partitions.is_empty() {
                    continue;
                }
                let system = is_system_disk(d);
                egui::Frame::new().stroke(Stroke::new(1.0_f32, pal.stroke)).corner_radius(6).inner_margin(8).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    // Header: model, device, size and what kind of disk it is.
                    ui.horizontal(|ui| {
                        let whole = dirs.iter().position(|e| e.0 == Path::new(&format!("/dev/{}", d.dev)));
                        match whole.filter(|w| mode == PickMode::FakeQuick && self.raw_target(&dirs[*w].0).is_some()) {
                            Some(w) => {
                                // The whole drive can be chosen for the quick test.
                                let text = RichText::new(format!("{} - {}", d.model, t(lang, "pick_whole"))).strong();
                                if ui.selectable_label(*idx == w, text).clicked() {
                                    *idx = w;
                                }
                            }
                            None => {
                                ui.label(RichText::new(&d.model).strong());
                            }
                        }
                        ui.label(RichText::new(format!("/dev/{} · {}", d.dev, self.bytes_text(d.size_bytes))).color(pal.muted));
                        let (tag, color) = if system {
                            (t(lang, "pick_system"), pal.caution)
                        } else if removable(d) {
                            (t(lang, "d_removable"), pal.good)
                        } else {
                            (t(lang, "pick_internal"), pal.unknown)
                        };
                        ui.label(RichText::new(tag).small().strong().color(color));
                    });
                    // The block bar: one block per partition, as wide as it is big.
                    let total = d.size_bytes.max(d.partitions.iter().map(|p| p.size).sum::<u64>()).max(1) as f32;
                    let width = ui.available_width();
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::hover());
                    ui.painter().rect_filled(rect, 3.0, pal.stripe);
                    let mut x = rect.left();
                    let gap = 2.0;
                    let n = d.partitions.len() as f32;
                    let usable = (width - gap * (n - 1.0)).max(10.0);
                    for p in &d.partitions {
                        let w = (usable * p.size as f32 / total).max(8.0);
                        let seg = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(w.min(rect.right() - x).max(2.0), rect.height()));
                        x += w + gap;
                        let entry = entry_of(p, dirs);
                        let access = access_of(entry, dirs);
                        let base = self.fs_color(&p.fs);
                        let dim = if access == Access::None { 0.35 } else { 1.0 };
                        let tint = |c: Color32, a: f32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (255.0 * a * dim) as u8);
                        ui.painter().rect_filled(seg, 3.0, tint(base, 0.30));
                        if let Some(used) = p.used {
                            let frac = (used as f32 / p.size.max(1) as f32).clamp(0.0, 1.0);
                            let filled = egui::Rect::from_min_size(seg.min, Vec2::new(seg.width() * frac, seg.height()));
                            ui.painter().rect_filled(filled, 3.0, tint(base, 0.85));
                        }
                        let selected = entry == Some(*idx) && entry.is_some();
                        if selected {
                            ui.painter().rect_stroke(seg, 3.0, Stroke::new(2.5_f32, pal.text), egui::StrokeKind::Outside);
                        }
                        let tip = format!("{} {} · {} · {}", p.name, p.label, p.fs, self.bytes_text(p.size));
                        let resp = ui.interact(seg, ui.id().with(("seg", &d.dev, &p.name)), Sense::click()).on_hover_text(tip);
                        if resp.clicked() {
                            if let Some(i) = entry {
                                *idx = i;
                            }
                        }
                    }
                    ui.add_space(4.0);
                    // The partitions, one per line.
                    for p in &d.partitions {
                        let entry = entry_of(p, dirs);
                        if let Some(i) = entry {
                            matched[i] = true;
                        }
                        let access = access_of(entry, dirs);
                        let (status, color) = match access {
                            Access::Ready => (t(lang, "pick_ok"), pal.good),
                            Access::Locked => (t(lang, "pick_locked"), pal.bad),
                            Access::Unmounted => (t(lang, "pick_unmounted"), pal.caution),
                            Access::None => (t(lang, "pick_none"), pal.muted),
                        };
                        let free = p.free.map(|f| format!(" · {} {}", self.bytes_text(f), t(lang, "col_free").to_lowercase())).unwrap_or_default();
                        let name = if p.label.is_empty() { p.name.clone() } else { format!("{} ({})", p.name, p.label) };
                        let text = format!("{name} · {} · {}{free}", if p.fs.is_empty() { "?" } else { &p.fs }, self.bytes_text(p.size));
                        ui.horizontal(|ui| {
                            let selected = entry.is_some() && entry == Some(*idx);
                            let resp = ui.add_enabled(entry.is_some(), egui::SelectableLabel::new(selected, RichText::new(text).monospace().size(12.5)));
                            if resp.clicked() {
                                if let Some(i) = entry {
                                    *idx = i;
                                }
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(status).small().strong().color(color));
                            });
                        });
                    }
                });
                ui.add_space(4.0);
            }
            // Folders that belong to no listed disk (the documents folder, other mounts).
            let others: Vec<usize> = (0..dirs.len()).filter(|i| !matched[*i] && !dirs[*i].0.starts_with("/dev/")).collect();
            if !others.is_empty() {
                ui.label(RichText::new(t(lang, "pick_others")).strong());
                for i in others {
                    let access = access_of(Some(i), dirs);
                    let label = crate::bench_ui::dir_label(lang, &dirs[i].1);
                    let short = label.split(" - ").next().unwrap_or(&label).to_string();
                    ui.horizontal(|ui| {
                        if ui.add(egui::SelectableLabel::new(*idx == i, RichText::new(short).monospace().size(12.5))).clicked() {
                            *idx = i;
                        }
                        if access == Access::Locked {
                            ui.label(RichText::new(t(lang, "pick_locked")).small().strong().color(pal.bad));
                        }
                    });
                }
            }
        });

        // What the test will do with the chosen entry.
        if let Some((dir, _)) = dirs.get(*idx) {
            let (text, color) = self.pick_will(dir, dirs.get(*idx).map(|d| d.1.as_str()).unwrap_or(""), mode);
            egui::Frame::new().fill(color.gamma_multiply(0.18)).stroke(Stroke::new(1.0_f32, color)).corner_radius(6).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(text).color(pal.text));
            });
        } else {
            ui.label(RichText::new(t(lang, "bench_no_dirs")).color(pal.muted));
        }
        refresh
    }

    /// The sentence that says what will happen, and its colour (green: nothing at risk).
    fn pick_will(&self, dir: &Path, label: &str, mode: PickMode) -> (String, Color32) {
        let lang = self.settings.language;
        let pal = self.pal();
        let shown = dir.display().to_string();
        if label.ends_with(crate::bench_ui::WHOLE) && !(mode == PickMode::FakeQuick && self.raw_target(dir).is_some()) {
            return (t(lang, "pick_will_whole").to_string(), pal.caution);
        }
        if label.ends_with(" [u]") && !(mode == PickMode::FakeQuick && self.raw_target(dir).is_some()) {
            return (t(lang, "pick_will_mount").to_string(), pal.caution);
        }
        if dir_label_locked(label) && !label.ends_with(" [u]") && !label.ends_with(crate::bench_ui::WHOLE) {
            return (t(lang, "bench_locked").to_string(), pal.bad);
        }
        if mode == PickMode::FakeQuick {
            if let Some((dev, _, unmount)) = self.raw_target(dir) {
                let parts = if unmount.is_empty() { "-".to_string() } else { unmount.join(", ") };
                return (t(lang, "pick_will_raw").replace("{dev}", &dev).replace("{parts}", &parts), pal.caution);
            }
        }
        let key = if mode == PickMode::Speed { "pick_will_speed" } else { "pick_will_files" };
        (t(lang, key).replace("{dir}", &shown), pal.good)
    }
}

/// A grid of blocks that fills as the fake-drive test goes: blue written, green checked, red wrong.
pub(crate) fn progress_blocks(ui: &mut egui::Ui, pal: &crate::theme::Palette, s: &crate::fakecheck::State, span_bytes: u64) {
    use crate::fakecheck::{Verdict, BLOCK};
    const COLS: usize = 50;
    const ROWS: usize = 3;
    const CELLS: usize = COLS * ROWS;
    let total = s.total.max(1) as f32;
    let written = s.written as f32 / total;
    let verified = if s.written > 0 { s.verified as f32 / s.written as f32 * written } else { 0.0 };
    let span_blocks = (span_bytes / BLOCK as u64).max(1) as f32;
    let mut red = [false; CELLS];
    if let Some(Verdict::Fake { real_bytes }) = &s.verdict {
        let from = (*real_bytes as f32 / span_bytes.max(1) as f32 * CELLS as f32).ceil() as usize;
        for r in red.iter_mut().skip(from.min(CELLS)) {
            *r = true;
        }
    }
    for b in &s.bad {
        let c = ((b.index as f32 / span_blocks) * CELLS as f32) as usize;
        red[c.min(CELLS - 1)] = true;
    }
    let width = ui.available_width();
    let cell = ((width - (COLS as f32 - 1.0) * 2.0) / COLS as f32).clamp(4.0, 14.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(COLS as f32 * (cell + 2.0), ROWS as f32 * (cell + 2.0)), Sense::hover());
    let blue = pal.accent;
    for c in 0..CELLS {
        let pos = (c as f32 + 0.5) / CELLS as f32;
        let color = if red[c] && pos <= verified.max(if s.verdict.is_some() { 1.0 } else { 0.0 }) {
            pal.bad
        } else if pos <= verified {
            pal.good
        } else if pos <= written {
            blue
        } else {
            pal.stripe
        };
        let (col, row) = (c % COLS, c / COLS);
        let r = egui::Rect::from_min_size(rect.min + Vec2::new(col as f32 * (cell + 2.0), row as f32 * (cell + 2.0)), Vec2::splat(cell));
        ui.painter().rect_filled(r, 2.0, color);
    }
}
