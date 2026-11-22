//! The card to the right of the disk details, when the window is wide: what the disk is reading
//! and writing right now, and the two tests (speed and fake drive) as large buttons.

use crate::disks::Disk;
use crate::theme::Palette;
use crate::App;
use eframe::egui::{self, Color32, RichText, Sense, Stroke};

/// What the person asked for from the card.
pub enum SummaryAction {
    Speed,
    Fake,
}

/// Write speeds are shown in this colour, reads in the theme's accent.
const WRITE: Color32 = Color32::from_rgb(235, 140, 30);

/// A box with a small title over a large number.
/// Its size never depends on the number shown, so nothing around it moves when the speed changes.
fn stat(ui: &mut egui::Ui, pal: &Palette, width: f32, title: &str, value: &str, color: Color32) {
    let size = egui::vec2(width, 58.0);
    ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
    ui.set_min_size(size);
    ui.set_max_size(size);
    egui::Frame::new().fill(pal.value_bg).stroke(Stroke::new(1.0_f32, pal.stroke)).corner_radius(6).inner_margin(8).show(ui, |ui| {
        ui.set_width(width - 18.0);
        ui.set_height(58.0 - 18.0);
        ui.vertical(|ui| {
            // "lectura" -> "Lectura"
            let mut t = title.chars();
            let title: String = t.next().map(|c| c.to_uppercase().chain(t).collect()).unwrap_or_default();
            ui.label(RichText::new(title).small().color(pal.muted));
            ui.add(egui::Label::new(RichText::new(value).size(20.0).strong().monospace().color(color)).truncate());
        });
    });
    });
}

/// A large button: a title and a line saying what it does. Returns true when clicked.
fn tile(ui: &mut egui::Ui, pal: &Palette, title: &str, subtitle: &str) -> bool {
    let inner = egui::Frame::new().fill(pal.tab_bg).stroke(Stroke::new(1.0_f32, pal.stroke)).corner_radius(8).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).size(15.0).strong().color(pal.text));
        ui.add(egui::Label::new(RichText::new(subtitle).small().color(pal.muted)).wrap().selectable(false));
    });
    let rect = inner.response.rect;
    let resp = ui.interact(rect, inner.response.id.with("tile"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.hovered() {
        ui.painter().rect_stroke(rect, 8.0, Stroke::new(2.0_f32, pal.accent), egui::StrokeKind::Inside);
    }
    resp.clicked()
}

impl App {
    pub(crate) fn summary_panel(&self, ui: &mut egui::Ui, d: &Disk, pal: &Palette) -> Option<SummaryAction> {
        let mut action = None;
        let width = ui.available_width();

        // Activity right now (the same reading as the line under the title, larger).
        ui.label(RichText::new(self.tr("activity")).strong());
        let sp = self.speeds.lock().ok().and_then(|m| m.get(&crate::speed::key_for(d)).copied());
        let rate = |b: Option<f64>| match b {
            Some(v) => format!("{}/s", self.bytes_text(v as u64)),
            None => "—".into(),
        };
        let half = (width - 8.0) / 2.0;
        match sp {
            Some(s) if s.total.is_some() => stat(ui, pal, width, self.tr("activity"), &rate(s.total), pal.accent),
            _ => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    stat(ui, pal, half, self.tr("act_read"), &rate(sp.and_then(|s| s.read)), pal.accent);
                    stat(ui, pal, half, self.tr("act_write"), &rate(sp.and_then(|s| s.write)), WRITE);
                });
            }
        }
        ui.add_space(10.0);

        // The tests.
        ui.label(RichText::new(self.tr("sum_tools")).strong());
        if tile(ui, pal, self.tr("bench_menu").trim_end_matches('…'), self.tr("sum_speed_sub")) {
            action = Some(SummaryAction::Speed);
        }
        ui.add_space(4.0);
        if tile(ui, pal, self.tr("fake_menu").trim_end_matches('…'), self.tr("sum_fake_sub")) {
            action = Some(SummaryAction::Fake);
        }
        action
    }
}
