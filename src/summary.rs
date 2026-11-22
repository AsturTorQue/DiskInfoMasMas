//! The summary card to the right of the disk details, when the window is wide: how full the
//! disk is (partition by partition), its temperature over the last days, what has been written
//! to it, and shortcuts to the speed and fake-drive tests.

use crate::disks::Disk;
use crate::theme::Palette;
use crate::App;
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};

/// What the person asked for from the card.
pub enum SummaryAction {
    Speed,
    Fake,
}

impl App {
    pub(crate) fn summary_panel(&self, ui: &mut egui::Ui, d: &Disk, pal: &Palette) -> Option<SummaryAction> {
        let mut action = None;
        egui::Frame::new().stroke(Stroke::new(1.0_f32, pal.stroke)).corner_radius(6).inner_margin(10).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let width = ui.available_width();

            // Space: one block per partition, darker for the part in use.
            ui.label(RichText::new(self.tr("sum_space")).strong());
            let total = d.size_bytes.max(d.partitions.iter().map(|p| p.size).sum::<u64>()).max(1);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
            ui.painter().rect_filled(rect, 3.0, pal.stripe);
            let mut x = rect.left();
            for p in &d.partitions {
                let w = (width * p.size as f32 / total as f32).max(2.0).min(rect.right() - x);
                let seg = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(w, rect.height()));
                x += w + 1.0;
                ui.painter().rect_filled(seg, 2.0, pal.accent.gamma_multiply(0.3));
                if let Some(used) = p.used {
                    let frac = (used as f32 / p.size.max(1) as f32).clamp(0.0, 1.0);
                    ui.painter().rect_filled(egui::Rect::from_min_size(seg.min, Vec2::new(seg.width() * frac, seg.height())), 2.0, pal.accent);
                }
            }
            let used: u64 = d.partitions.iter().filter_map(|p| p.used).sum();
            let free: u64 = d.partitions.iter().filter_map(|p| p.free).sum();
            if used + free > 0 {
                let pct = used as f64 * 100.0 / (used + free) as f64;
                ui.label(
                    RichText::new(format!(
                        "{} {} · {} {} ({:.0} %)",
                        self.bytes_text(used),
                        self.tr("sum_used"),
                        self.bytes_text(free),
                        self.tr("col_free").to_lowercase(),
                        pct
                    ))
                    .color(pal.muted),
                );
            } else {
                ui.label(RichText::new(self.tr("sum_no_space")).color(pal.muted));
            }
            ui.add_space(8.0);

            // Temperature over the last 7 days, from the history the program keeps.
            ui.label(RichText::new(self.tr("sum_temp")).strong());
            let week_ago = crate::history::now().saturating_sub(7 * 86_400);
            let temps: Vec<f32> = self.samples_of(d).iter().filter(|s| s.t >= week_ago).filter_map(|s| s.temp).map(|t| t as f32).collect();
            if temps.len() >= 2 {
                let (lo, hi) = temps.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
                let span = (hi - lo).max(4.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 48.0), Sense::hover());
                ui.painter().rect_filled(rect, 3.0, pal.stripe);
                let n = temps.len() as f32;
                let pts: Vec<egui::Pos2> = temps
                    .iter()
                    .enumerate()
                    .map(|(i, v)| egui::pos2(rect.left() + rect.width() * i as f32 / (n - 1.0), rect.bottom() - 3.0 - (rect.height() - 6.0) * (v - lo) / span))
                    .collect();
                ui.painter().add(egui::Shape::line(pts, Stroke::new(1.6_f32, Color32::from_rgb(235, 140, 30))));
                ui.label(
                    RichText::new(format!("{} {} · {} {}", self.tr("sum_min"), self.temp_text(Some(lo as i32)), self.tr("sum_max"), self.temp_text(Some(hi as i32))))
                        .color(pal.muted),
                );
            } else {
                ui.label(RichText::new(self.tr("sum_no_history")).color(pal.muted));
            }
            ui.add_space(8.0);

            // What has gone through the disk.
            if d.host_writes_gb.is_some() || d.host_reads_gb.is_some() {
                ui.label(RichText::new(self.tr("sum_traffic")).strong());
                let tb = |gb: Option<u64>| gb.map(|g| self.bytes_text(g.saturating_mul(1_000_000_000))).unwrap_or_else(|| "—".into());
                ui.label(
                    RichText::new(format!("{} {} · {} {}", self.tr("act_write"), tb(d.host_writes_gb), self.tr("act_read"), tb(d.host_reads_gb)))
                        .color(pal.muted),
                );
                ui.add_space(8.0);
            }

            // Shortcuts.
            ui.label(RichText::new(self.tr("sum_tools")).strong());
            ui.horizontal_wrapped(|ui| {
                if ui.button(self.tr("bench_menu").trim_end_matches('…')).clicked() {
                    action = Some(SummaryAction::Speed);
                }
                if ui.button(self.tr("fake_menu").trim_end_matches('…')).clicked() {
                    action = Some(SummaryAction::Fake);
                }
            });
        });
        action
    }
}
