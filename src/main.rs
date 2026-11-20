// No console window behind the app on Windows (release builds).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod demo;
mod disks;
mod health;
mod parts;
mod i18n;
#[cfg(target_os = "linux")]
mod integrate;
mod platform;
#[cfg(target_os = "linux")]
mod polkit;
mod smartctl;
mod theme;

use disks::{Disk, ScanResult, Status};
use eframe::egui::{self, Color32, RichText, Vec2};
use health::Thresholds;
use i18n::{t, Lang};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use theme::{Palette, Theme};

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Settings {
    auto_refresh_min: u32,
    green_mode: bool,
    fahrenheit: bool,
    language: Lang,
    theme: Theme,
    thresholds: HashMap<String, Thresholds>,
    /// Keep an entry (with icon) in the user's application menu.
    menu_entry: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_refresh_min: 10,
            green_mode: false,
            fahrenheit: false,
            language: Lang::from_system(),
            theme: Theme::Light,
            thresholds: HashMap::new(),
            menu_entry: true,
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    Some(platform::config_dir()?.join("settings.json"))
}

impl Settings {
    fn load() -> Self {
        // Settings saved before the rename are picked up once, then saved in the new place.
        let legacy = || platform::legacy_config_dir().map(|d| d.join("settings.json"));
        settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .or_else(|| legacy().and_then(|p| std::fs::read_to_string(p).ok()))
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    fn save(&self) {
        if let Some(p) = settings_path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(t) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(p, t);
            }
        }
    }
    fn thresholds_for(&self, d: &Disk) -> Thresholds {
        self.thresholds.get(&d.key()).copied().unwrap_or_else(|| Thresholds::defaults(d.is_ssd))
    }
}

struct Shot {
    path: PathBuf,
    frames: u32,
    requested: bool,
}

struct App {
    disks: Vec<Disk>,
    selected: usize,
    settings: Settings,
    scanning: Option<Receiver<ScanResult>>,
    scan_admin: bool,
    needs_admin: bool,
    direct_ok: bool,
    message: Option<&'static str>,
    last_refresh: Instant,
    show_thresholds: bool,
    edit_th: Thresholds,
    show_about: bool,
    toast: Option<(&'static str, Instant)>,
    applied_theme: Option<Theme>,
    /// The passwordless rule let us read S.M.A.R.T. on the last scan.
    passwordless: bool,
    show_pw: bool,
    /// Installing or removing the rule: (turning on?, result).
    pw_task: Option<Receiver<(bool, Result<(), &'static str>)>>,
    /// Which view is shown under the disk facts.
    tab: Tab,
    shot: Option<Shot>,
}

pub const APP_NAME: &str = "DiskInfo";

const AUTO_REFRESH_CHOICES: [u32; 8] = [0, 1, 3, 5, 10, 30, 60, 120];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Smart,
    Partitions,
    Details,
}

impl App {
    fn new(shot: Option<PathBuf>, lang: Option<Lang>, theme: Option<Theme>, demo: bool) -> Self {
        let mut settings = Settings::load();
        if let Some(l) = lang {
            settings.language = l;
        }
        if let Some(th) = theme {
            settings.theme = th;
        }
        let mut app = Self {
            disks: Vec::new(),
            selected: 0,
            settings,
            scanning: None,
            scan_admin: false,
            needs_admin: false,
            direct_ok: false,
            message: None,
            last_refresh: Instant::now(),
            show_thresholds: false,
            edit_th: Thresholds::defaults(false),
            show_about: false,
            toast: None,
            applied_theme: None,
            passwordless: false,
            show_pw: false,
            pw_task: None,
            tab: Tab::Smart,
            shot: shot.map(|path| Shot { path, frames: 0, requested: false }),
        };
        if demo {
            app.apply_scan(demo::demo_scan(), false);
        } else if app.shot.is_some() {
            // Read synchronously so the picture shows real disks.
            let r = disks::scan(false);
            app.apply_scan(r, false);
        } else {
            app.start_scan(false);
        }
        app
    }

    fn tr(&self, key: &'static str) -> &'static str {
        t(self.settings.language, key)
    }

    fn pal(&self) -> Palette {
        self.settings.theme.palette()
    }

    fn start_scan(&mut self, admin: bool) {
        if self.scanning.is_some() {
            return;
        }
        self.scan_admin = admin;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(disks::scan(admin));
        });
        self.scanning = Some(rx);
    }

    fn apply_scan(&mut self, r: ScanResult, admin: bool) {
        let keep = self.disks.get(self.selected).map(|d| d.dev.clone());
        // An admin read that was refused keeps the data we already had.
        if admin && r.needs_admin && !self.disks.is_empty() {
            self.message = r.message;
            return;
        }
        self.disks = r.disks;
        if !admin {
            self.direct_ok = self.disks.iter().any(|d| d.smart_read);
            self.passwordless = r.passwordless;
        }
        self.needs_admin = r.needs_admin && !self.disks.iter().any(|d| d.smart_read);
        self.message = r.message;
        self.selected = keep
            .and_then(|k| self.disks.iter().position(|d| d.dev == k))
            .unwrap_or(0);
        self.reevaluate();
        self.last_refresh = Instant::now();
    }

    fn reevaluate(&mut self) {
        for d in &mut self.disks {
            let th = self.settings.thresholds_for(d);
            health::evaluate(d, &th);
        }
    }

    fn auto_refresh(&mut self) {
        if self.direct_ok {
            self.start_scan(false);
        } else {
            for d in &mut self.disks {
                if let Some(t) = disks::hwmon_temp(&d.dev) {
                    d.temperature = Some(t);
                }
            }
            self.last_refresh = Instant::now();
        }
    }

    /// Reads S.M.A.R.T. with administrator rights: one password prompt on Linux and macOS;
    /// on Windows the program restarts itself elevated (UAC) and this copy closes.
    fn read_as_admin(&mut self) {
        #[cfg(windows)]
        {
            if platform::relaunch_as_admin() {
                std::process::exit(0);
            }
        }
        #[cfg(not(windows))]
        self.start_scan(true);
    }

    fn refresh(&mut self) {
        let admin = !self.direct_ok && self.disks.iter().any(|d| d.smart_read);
        self.start_scan(admin);
    }

    fn temp_text(&self, t: Option<i32>) -> String {
        match t {
            Some(c) if c != health::TEMP_UNKNOWN => {
                if self.settings.fahrenheit {
                    format!("{} °F", c * 9 / 5 + 32)
                } else {
                    format!("{c} °C")
                }
            }
            _ => "-- °C".into(),
        }
    }

    fn status_colors(&self, s: Status) -> (Color32, Color32) {
        let p = self.pal();
        match s {
            Status::Good if self.settings.green_mode => (Color32::from_rgb(46, 170, 87), Color32::WHITE),
            Status::Good => (p.good, Color32::WHITE),
            Status::Caution => (p.caution, Color32::from_rgb(40, 30, 0)),
            Status::Bad => (p.bad, Color32::WHITE),
            Status::Unknown => (p.unknown, Color32::WHITE),
        }
    }

    fn status_word(&self, s: Status) -> &'static str {
        self.tr(match s {
            Status::Good => "good",
            Status::Caution => "caution",
            Status::Bad => "bad",
            Status::Unknown => "unknown",
        })
    }

    fn health_text(&self, d: &Disk) -> String {
        match d.life {
            Some(l) if d.health != Status::Unknown => format!("{} ({l} %)", self.status_word(d.health)),
            _ => self.status_word(d.health).to_string(),
        }
    }

    fn size_text(&self, bytes: u64) -> String {
        let gb = bytes as f64 / 1e9;
        let s = if gb >= 10_000.0 { format!("{:.1} TB", gb / 1000.0) } else { format!("{gb:.1} GB") };
        if self.settings.language.decimal_comma() {
            s.replace('.', ",")
        } else {
            s
        }
    }

    fn attr_name(&self, d: &Disk, a: &disks::Attr) -> String {
        i18n::attr_name(self.settings.language, a.id, d.is_nvme, &a.name)
    }

    fn report(&self) -> String {
        let mut out = format!("{}\n\n", self.tr("report_title"));
        let hours = format!(" {}", self.tr("unit_hours"));
        let times = format!(" {}", self.tr("unit_times"));
        for (i, d) in self.disks.iter().enumerate() {
            let line = "-".repeat(76);
            out += &format!("{line}\n ({}) {}\n{line}\n", i + 1, d.model);
            let rows = [
                (self.tr("model"), d.model.clone()),
                (self.tr("firmware"), d.firmware.clone()),
                (self.tr("serial"), d.serial.clone()),
                (self.tr("capacity"), self.size_text(d.size_bytes)),
                (self.tr("interface"), d.interface.clone()),
                (self.tr("standard"), d.standard.clone()),
                (self.tr("transfer_mode"), d.transfer_mode.clone()),
                (self.tr("power_on_hours"), opt(d.power_on_hours, &hours)),
                (self.tr("power_on_count"), opt(d.power_on_count, &times)),
                (self.tr("host_reads"), opt(d.host_reads_gb, " GB")),
                (self.tr("host_writes"), opt(d.host_writes_gb, " GB")),
                (self.tr("temperature"), self.temp_text(d.temperature)),
                (self.tr("health_status"), self.health_text(d)),
                (self.tr("features"), d.features.join(", ")),
                (self.tr("mounts"), d.mounts.join(" ")),
            ];
            for (k, v) in rows {
                out += &format!("{k:>26} : {v}\n");
            }
            for (k, v) in &d.details {
                out += &format!("{:>26} : {}\n", self.tr(k), self.value_text(v));
            }
            if !d.partitions.is_empty() {
                out += &format!("\n-- {} ---------------------------------------------------------\n", self.tr("tab_parts"));
                for p in &d.partitions {
                    let usage = p
                        .used_fraction()
                        .map(|f| format!("{:.0} %", f * 100.0))
                        .unwrap_or_else(|| "—".into());
                    out += &format!(
                        "{:<14} {:>10} {:<18} {:>6}  {:<16} {}  {}\n",
                        p.name,
                        self.bytes_text(p.size),
                        p.fs,
                        usage,
                        p.label,
                        p.kind,
                        p.mounts.join(" ")
                    );
                }
            }
            if !d.attrs.is_empty() {
                out += "\n-- S.M.A.R.T. ---------------------------------------------------------\n";
                out += &format!(
                    "ID {} | {} | {} | {} | {}\n",
                    self.tr("col_current"),
                    self.tr("col_worst"),
                    self.tr("col_threshold"),
                    self.tr("col_raw"),
                    self.tr("col_name")
                );
                for a in &d.attrs {
                    out += &format!(
                        "{:02X} {:>3} {:>4} {:>3} {:012X} {}\n",
                        a.id,
                        a.current.map(|v| v.to_string()).unwrap_or_default(),
                        a.worst.map(|v| v.to_string()).unwrap_or_default(),
                        a.thresh.map(|v| v.to_string()).unwrap_or_default(),
                        a.raw,
                        self.attr_name(d, a)
                    );
                }
            }
            out += "\n";
        }
        out
    }
}

fn opt(v: Option<u64>, unit: &str) -> String {
    v.map(|n| format!("{n}{unit}")).unwrap_or_else(|| "----".into())
}

/// A rounded coloured box with centred text, like the original's status panels.
fn status_box(ui: &mut egui::Ui, size: Vec2, fill: Color32, fg: Color32, lines: &[(String, f32)], tip: &str) {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    resp.on_hover_text(tip);
    let p = ui.painter();
    p.rect_filled(rect, 6.0, fill);
    let top = Color32::from_white_alpha(28);
    p.rect_filled(egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.center().y)), egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, top);
    p.rect_stroke(rect, 6.0, egui::Stroke::new(1.0_f32, fill.gamma_multiply(0.7)), egui::StrokeKind::Inside);
    let total: f32 = lines.iter().map(|(_, s)| s * 1.25).sum();
    let mut y = rect.center().y - total / 2.0;
    for (t, s) in lines {
        let h = s * 1.25;
        p.text(egui::pos2(rect.center().x, y + h / 2.0), egui::Align2::CENTER_CENTER, t, egui::FontId::proportional(*s), fg);
        y += h;
    }
}

/// A sunken value field. Resting the pointer on it shows `full` (the whole value,
/// which may be cut in the field) followed by the field's help.
fn value_box(ui: &mut egui::Ui, pal: &Palette, text: &str, width: f32, full: &str, help: &str) {
    let resp = egui::Frame::new()
        .fill(pal.value_bg)
        .stroke(egui::Stroke::new(1.0_f32, pal.stroke))
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.set_width(width);
            ui.add(egui::Label::new(RichText::new(text).monospace().size(13.0)).truncate().selectable(false));
        })
        .response
        .interact(egui::Sense::hover());
    resp.on_hover_ui(|ui| {
        ui.set_max_width(380.0);
        if !full.is_empty() && full != "—" && full != "----" {
            ui.label(RichText::new(full).monospace().strong());
            ui.add_space(4.0);
        }
        ui.label(help);
    });
}

/// A field name; resting the pointer on it shows the help.
fn help_label(ui: &mut egui::Ui, text: &str, help: &str) {
    let resp = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.label(text);
        })
        .response
        .interact(egui::Sense::hover());
    resp.on_hover_ui(|ui| {
        ui.set_max_width(380.0);
        ui.label(help);
    });
}

fn right_label(ui: &mut egui::Ui, text: &str, help: &str) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let resp = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.label(text);
            })
            .response
            .interact(egui::Sense::hover());
        resp.on_hover_ui(|ui| {
            ui.set_max_width(380.0);
            ui.label(help);
        });
    });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.applied_theme != Some(self.settings.theme) {
            ctx.set_visuals(self.pal().visuals());
            self.applied_theme = Some(self.settings.theme);
        }
        let pal = self.pal();

        // Background scan results.
        if let Some(rx) = &self.scanning {
            if let Ok(r) = rx.try_recv() {
                let admin = self.scan_admin;
                self.scanning = None;
                self.apply_scan(r, admin);
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        let every = self.settings.auto_refresh_min;
        if every > 0 && self.scanning.is_none() && self.last_refresh.elapsed() >= Duration::from_secs(every as u64 * 60) {
            self.auto_refresh();
        }
        ctx.request_repaint_after(Duration::from_secs(5));
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.refresh();
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button(self.tr("menu_file"), |ui| {
                    #[cfg(target_os = "linux")]
                    {
                        let label = self.tr("menu_entry");
                        if ui.checkbox(&mut self.settings.menu_entry, label).changed() {
                            if self.settings.menu_entry {
                                integrate::install(self.tr("about_1"));
                            } else {
                                integrate::uninstall();
                            }
                            self.settings.save();
                        }
                        ui.separator();
                    }
                    if ui.button(self.tr("quit")).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button(self.tr("menu_edit"), |ui| {
                    if ui.button(self.tr("copy_report")).clicked() {
                        ctx.copy_text(self.report());
                        self.toast = Some(("report_copied", Instant::now()));
                        ui.close_menu();
                    }
                });
                ui.menu_button(self.tr("menu_function"), |ui| {
                    if ui.button(self.tr("refresh")).clicked() {
                        self.refresh();
                        ui.close_menu();
                    }
                    if ui.button(self.tr("read_admin")).clicked() {
                        self.read_as_admin();
                        ui.close_menu();
                    }
                    ui.menu_button(self.tr("auto_refresh"), |ui| {
                        for m in AUTO_REFRESH_CHOICES {
                            let label = if m == 0 { self.tr("off").to_string() } else { format!("{m} min") };
                            if ui.radio(self.settings.auto_refresh_min == m, label).clicked() {
                                self.settings.auto_refresh_min = m;
                                self.settings.save();
                                ui.close_menu();
                            }
                        }
                    });
                    if cfg!(target_os = "linux") && ui.button(self.tr("pw_menu")).clicked() {
                        self.show_pw = true;
                        ui.close_menu();
                    }
                    if ui.button(self.tr("thresholds_menu")).clicked() {
                        if let Some(d) = self.disks.get(self.selected) {
                            self.edit_th = self.settings.thresholds_for(d);
                            self.show_thresholds = true;
                        }
                        ui.close_menu();
                    }
                });
                ui.menu_button(self.tr("menu_theme"), |ui| {
                    for th in Theme::ALL {
                        if ui.radio(self.settings.theme == th, self.tr(th.name_key())).clicked() {
                            self.settings.theme = th;
                            self.settings.save();
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    let (green, fahrenheit) = (self.tr("green_mode"), self.tr("fahrenheit"));
                    if ui.checkbox(&mut self.settings.green_mode, green).changed() {
                        self.settings.save();
                    }
                    if ui.checkbox(&mut self.settings.fahrenheit, fahrenheit).changed() {
                        self.settings.save();
                    }
                });
                ui.menu_button(self.tr("menu_disk"), |ui| {
                    for i in 0..self.disks.len() {
                        let label = format!("({}) {} — {}", i + 1, self.disks[i].model, self.disks[i].place());
                        if ui.radio(self.selected == i, label).clicked() {
                            self.selected = i;
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button(self.tr("menu_language"), |ui| {
                    for l in Lang::ALL {
                        if ui.radio(self.settings.language == l, l.native_name()).clicked() {
                            self.settings.language = l;
                            self.settings.save();
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button(self.tr("menu_help"), |ui| {
                    if ui.button(self.tr("about")).clicked() {
                        self.show_about = true;
                        ui.close_menu();
                    }
                });
                if self.scanning.is_some() {
                    ui.spinner();
                    ui.label(self.tr("reading"));
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.needs_admin {
                egui::Frame::new()
                    .fill(pal.banner_bg)
                    .stroke(egui::Stroke::new(1.0_f32, pal.banner_stroke))
                    .inner_margin(6)
                    .corner_radius(4)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            let (text, button) = if cfg!(windows) {
                                ("admin_banner_win", "restart_admin")
                            } else {
                                ("admin_banner", "read_smart_btn")
                            };
                            ui.label(RichText::new(self.tr(text)).color(pal.banner_text));
                            if ui.add_enabled(self.scanning.is_none(), egui::Button::new(self.tr(button))).clicked() {
                                self.read_as_admin();
                            }
                            if cfg!(target_os = "linux") && ui.button(self.tr("pw_banner_btn")).clicked() {
                                self.show_pw = true;
                            }
                        });
                    });
            }
            if let Some(m) = self.message {
                ui.colored_label(pal.error, self.tr(m));
            }
            if self.disks.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| ui.heading(self.tr("no_disks")));
                return;
            }

            // Disk tabs.
            egui::ScrollArea::horizontal().id_salt("tabs").show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    for i in 0..self.disks.len() {
                        let d = &self.disks[i];
                        let th = self.settings.thresholds_for(d);
                        let (fill, _) = self.status_colors(d.health);
                        let (tfill, _) = self.status_colors(health::temperature_status(d.temperature, &th));
                        let place = d.mounts.first().cloned().unwrap_or_else(|| d.place());
                        let sel = i == self.selected;
                        let resp = egui::Frame::new()
                            .fill(if sel { pal.tab_sel_bg } else { pal.tab_bg })
                            .stroke(egui::Stroke::new(1.0_f32, if sel { pal.accent } else { pal.stroke }))
                            .corner_radius(4)
                            .inner_margin(egui::Margin::symmetric(8, 4))
                            .show(ui, |ui| {
                                ui.set_min_width(96.0);
                                ui.horizontal(|ui| {
                                    let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                                    ui.painter().circle_filled(r.center(), 6.5, fill);
                                    ui.vertical(|ui| {
                                        ui.spacing_mut().item_spacing.y = 0.0;
                                        ui.label(RichText::new(self.status_word(d.health)).strong().size(12.0));
                                        ui.label(RichText::new(self.temp_text(d.temperature)).color(tfill).size(12.0));
                                        ui.add(egui::Label::new(RichText::new(place).size(11.0).color(pal.muted)).truncate());
                                    });
                                });
                            })
                            .response
                            .interact(egui::Sense::click());
                        let mounts = if d.mounts.is_empty() { "—".to_string() } else { d.mounts.join("\n") };
                        let tab_tip = format!("{}\n{}\n{}\n\n{}", d.model, d.place(), mounts, self.tr("h_tab"));
                        let resp = resp.on_hover_text(tab_tip);
                        if resp.clicked() {
                            self.selected = i;
                        }
                    }
                });
            });
            ui.add_space(4.0);

            let d = self.disks[self.selected.min(self.disks.len() - 1)].clone();
            let th = self.settings.thresholds_for(&d);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(format!("{} {}", d.model, self.size_text(d.size_bytes))).size(20.0).strong());
            });
            ui.add_space(6.0);

            let hours = format!(" {}", self.tr("unit_hours"));
            let times = format!(" {}", self.tr("unit_times"));
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(130.0);
                    help_label(ui, self.tr("health_status"), self.tr("h_health"));
                    let (f, fg) = self.status_colors(d.health);
                    let mut lines = vec![(self.status_word(d.health).to_string(), 20.0)];
                    if let (Some(l), true) = (d.life, d.health != Status::Unknown) {
                        lines.push((format!("{l} %"), 16.0));
                    }
                    let health_tip = format!("{}\n\n{}", self.health_text(&d), self.tr("h_health"));
                    status_box(ui, Vec2::new(128.0, 64.0), f, fg, &lines, &health_tip);
                    ui.add_space(8.0);
                    help_label(ui, self.tr("temperature"), self.tr("h_temperature"));
                    let (f, fg) = self.status_colors(health::temperature_status(d.temperature, &th));
                    let temp_tip = format!(
                        "{} ({} {} °C)\n\n{}",
                        self.temp_text(d.temperature),
                        self.tr("th_temp").split(" (").next().unwrap_or(""),
                        th.alarm_temp,
                        self.tr("h_temperature")
                    );
                    status_box(ui, Vec2::new(128.0, 46.0), f, fg, &[(self.temp_text(d.temperature), 20.0)], &temp_tip);
                });
                ui.add_space(8.0);
                let w = ((ui.available_width() - 340.0) / 2.0).max(90.0);
                egui::Grid::new("info").num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
                    let rotation = d
                        .rotation_rpm
                        .map(|r| format!("{r} RPM"))
                        .unwrap_or_else(|| if d.is_ssd { "---- (SSD)".into() } else { "----".into() });
                    let mounts_short = if d.mounts.is_empty() { "—".to_string() } else { d.mounts.join(" ") };
                    let mounts_full = d.mounts.join("\n");
                    // (name, value, full value, help) for the left and right columns.
                    type Field<'a> = (&'a str, String, String, &'a str);
                    let f = |name: &'static str, v: String, help: &'static str| -> Field {
                        (self.tr(name), v.clone(), v, self.tr(help))
                    };
                    let rows: [(Field, Field); 5] = [
                        (f("firmware", d.firmware.clone(), "h_firmware"), f("host_reads", opt(d.host_reads_gb, " GB"), "h_host_reads")),
                        (f("serial", d.serial.clone(), "h_serial"), f("host_writes", opt(d.host_writes_gb, " GB"), "h_host_writes")),
                        (f("interface", d.interface.clone(), "h_interface"), f("rotation", rotation, "h_rotation")),
                        (f("transfer_mode", d.transfer_mode.clone(), "h_transfer_mode"), f("power_on_count", opt(d.power_on_count, &times), "h_power_on_count")),
                        (
                            (self.tr("mounts"), mounts_short, mounts_full, self.tr("h_mounts")),
                            f("power_on_hours", opt(d.power_on_hours, &hours), "h_power_on_hours"),
                        ),
                    ];
                    for ((k1, v1, full1, h1), (k2, v2, full2, h2)) in rows {
                        right_label(ui, k1, h1);
                        value_box(ui, &pal, &v1, w, &full1, h1);
                        right_label(ui, k2, h2);
                        value_box(ui, &pal, &v2, 110.0, &full2, h2);
                        ui.end_row();
                    }
                });
            });
            for (k, v, h) in [
                (self.tr("standard"), d.standard.clone(), self.tr("h_standard")),
                (self.tr("features"), d.features.join(", "), self.tr("h_features")),
            ] {
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(Vec2::new(134.0, 20.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        right_label(ui, k, h);
                    });
                    let width = ui.available_width() - 16.0;
                    value_box(ui, &pal, if v.is_empty() { "—" } else { &v }, width, &v, h);
                });
            }
            ui.add_space(6.0);

            // Tabs: S.M.A.R.T. / Partitions / Details.
            ui.horizontal(|ui| {
                for (tab, key) in [(Tab::Smart, "tab_smart"), (Tab::Partitions, "tab_parts"), (Tab::Details, "tab_details")] {
                    let label = match tab {
                        Tab::Partitions => format!("{} ({})", self.tr(key), d.partitions.len()),
                        _ => self.tr(key).to_string(),
                    };
                    let text = RichText::new(label).strong();
                    if ui.selectable_label(self.tab == tab, text).clicked() {
                        self.tab = tab;
                    }
                }
            });
            egui::Frame::new()
                .stroke(egui::Stroke::new(1.0_f32, pal.stroke))
                .fill(pal.value_bg)
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                        Tab::Smart => self.smart_table(ui, &d, &pal),
                        Tab::Partitions => self.partitions_view(ui, &d, &pal),
                        Tab::Details => self.details_view(ui, &d),
                    });
                });
        });

        if self.show_thresholds {
            let mut open = true;
            let mut apply = false;
            egui::Window::new(self.tr("thresholds_title")).open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
                let d = &self.disks[self.selected];
                ui.label(format!("{} ({})", d.model, d.place()));
                ui.add_space(4.0);
                let lang = self.settings.language;
                egui::Grid::new("th").num_columns(2).show(ui, |ui| {
                    ui.label(t(lang, "th_05"));
                    ui.add(egui::DragValue::new(&mut self.edit_th.t05).range(0..=65535));
                    ui.end_row();
                    ui.label(t(lang, "th_c5"));
                    ui.add(egui::DragValue::new(&mut self.edit_th.tc5).range(0..=65535));
                    ui.end_row();
                    ui.label(t(lang, "th_c6"));
                    ui.add(egui::DragValue::new(&mut self.edit_th.tc6).range(0..=65535));
                    ui.end_row();
                    ui.label(t(lang, "th_ff"));
                    ui.add(egui::DragValue::new(&mut self.edit_th.tff).range(0..=100));
                    ui.end_row();
                    ui.label(t(lang, "th_temp"));
                    ui.add(egui::DragValue::new(&mut self.edit_th.alarm_temp).range(20..=100));
                    ui.end_row();
                });
                ui.small(t(lang, "th_zero_hint"));
                ui.horizontal(|ui| {
                    if ui.button(t(lang, "default")).clicked() {
                        self.edit_th = Thresholds::defaults(d.is_ssd);
                    }
                    if ui.button(t(lang, "apply")).clicked() {
                        apply = true;
                    }
                });
            });
            if apply {
                let key = self.disks[self.selected].key();
                self.settings.thresholds.insert(key, self.edit_th);
                self.settings.save();
                self.reevaluate();
            }
            self.show_thresholds = open;
        }

        // Result of turning the passwordless rule on or off.
        if let Some(rx) = &self.pw_task {
            if let Ok((enable, res)) = rx.try_recv() {
                self.pw_task = None;
                match res {
                    Ok(()) => {
                        self.toast = Some((if enable { "msg_rule_on" } else { "msg_rule_off" }, Instant::now()));
                        self.message = None;
                        self.start_scan(false);
                    }
                    Err(e) => self.message = Some(e),
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }

        if self.show_pw {
            let lang = self.settings.language;
            let busy = self.pw_task.is_some();
            let mut action: Option<bool> = None;
            egui::Window::new(t(lang, "pw_menu").trim_end_matches('…'))
                .open(&mut self.show_pw)
                .resizable(false)
                .collapsible(false)
                .default_width(460.0)
                .show(ctx, |ui| {
                    ui.label(t(lang, "pw_explain"));
                    ui.add_space(6.0);
                    ui.label(RichText::new(t(lang, if self.passwordless { "pw_state_on" } else { "pw_state_off" })).strong());
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!busy && !self.passwordless, egui::Button::new(t(lang, "pw_enable"))).clicked() {
                            action = Some(true);
                        }
                        if ui.add_enabled(!busy && self.passwordless, egui::Button::new(t(lang, "pw_disable"))).clicked() {
                            action = Some(false);
                        }
                        if busy {
                            ui.spinner();
                            ui.label(t(lang, "pw_working"));
                        }
                    });
                });
            #[cfg(target_os = "linux")]
            if let Some(enable) = action {
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let res = if enable { polkit::install() } else { polkit::uninstall() };
                    let _ = tx.send((enable, res));
                });
                self.pw_task = Some(rx);
            }
            #[cfg(not(target_os = "linux"))]
            let _ = action;
        }

        if self.show_about {
            let lang = self.settings.language;
            egui::Window::new(t(lang, "about")).open(&mut self.show_about).resizable(false).collapsible(false).show(ctx, |ui| {
                ui.heading(APP_NAME);
                ui.label(t(lang, "about_1"));
                ui.label(t(lang, "about_2"));
                ui.add_space(6.0);
                ui.label(t(lang, "about_credit"));
                ui.hyperlink("https://github.com/hiyohiyo/CrystalDiskInfo");
                ui.small("Copyright (c) 2008-2023 hiyohiyo");
            });
        }

        if let Some((key, at)) = self.toast {
            if at.elapsed() < Duration::from_secs(3) {
                let text = self.tr(key);
                egui::Area::new("toast".into()).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -16.0]).show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(text));
                });
                ctx.request_repaint_after(Duration::from_millis(200));
            } else {
                self.toast = None;
            }
        }

        self.screenshot_step(ctx);
    }
}

/// Height of a table row.
const ROW: f32 = 20.0;

impl App {
    /// "512 MB", "931,5 GB": decimal units like the disk size, with the language's decimal mark.
    fn bytes_text(&self, b: u64) -> String {
        let units = ["B", "KB", "MB", "GB", "TB", "PB"];
        let mut v = b as f64;
        let mut i = 0;
        while v >= 1000.0 && i < units.len() - 1 {
            v /= 1000.0;
            i += 1;
        }
        let s = if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", units[i]) };
        if self.settings.language.decimal_comma() {
            s.replace('.', ",")
        } else {
            s
        }
    }

    /// 4000787030016 → "4.000.787.030.016" (or with commas in English).
    fn group_digits(&self, n: u64) -> String {
        let sep = if self.settings.language.decimal_comma() { '.' } else { ',' };
        let digits = n.to_string();
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                out.push(sep);
            }
            out.push(c);
        }
        out
    }

    /// Translates the few values that are words ("yes", "passed"...).
    fn value_text(&self, v: &str) -> String {
        match v {
            "yes" | "no" | "passed" | "failed" => self.tr(match v {
                "yes" => "v_yes",
                "no" => "v_no",
                "passed" => "v_passed",
                _ => "v_failed",
            })
            .to_string(),
            other => other.to_string(),
        }
    }

    fn smart_table(&self, ui: &mut egui::Ui, d: &Disk, pal: &Palette) {
        egui::Grid::new("attrs").striped(true).num_columns(7).spacing([10.0, 2.0]).min_row_height(ROW).show(ui, |ui| {
            for (h, help) in [
                ("", ""),
                ("ID", ""),
                (self.tr("col_name"), ""),
                (self.tr("col_current"), self.tr("h_col_current")),
                (self.tr("col_worst"), self.tr("h_col_worst")),
                (self.tr("col_threshold"), self.tr("h_col_threshold")),
                (self.tr("col_raw"), self.tr("h_col_raw")),
            ] {
                let r = ui.label(RichText::new(h).strong());
                if !help.is_empty() {
                    r.on_hover_text(help);
                }
            }
            ui.end_row();
            if d.attrs.is_empty() {
                ui.label("");
                ui.label("");
                ui.label(RichText::new(self.tr("no_smart")).italics().color(pal.muted));
                ui.end_row();
            }
            let nvme = d.is_nvme;
            for a in &d.attrs {
                let (fill, _) = self.status_colors(a.status);
                // The dot sits in its own cell, centred on the row's height.
                let (r, _) = ui.allocate_exact_size(Vec2::new(16.0, ROW), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 5.0, fill);
                ui.monospace(format!("{:02X}", a.id));
                let name = self.attr_name(d, a);
                let mut tip = format!("{:02X} {name}", a.id);
                if let Some(h) = i18n::attr_help(self.settings.language, a.id, nvme) {
                    tip += &format!("\n{h}");
                }
                tip += &format!("\n\n{}: {}", self.tr("raw_decimal"), a.raw);
                ui.label(name).on_hover_text(tip);
                let n = |v: Option<u32>| v.map(|x| x.to_string()).unwrap_or_default();
                ui.monospace(n(a.current));
                ui.monospace(n(a.worst));
                ui.monospace(n(a.thresh));
                ui.monospace(if nvme { format!("{:014X}", a.raw) } else { format!("{:012X}", a.raw) });
                ui.end_row();
            }
        });
    }

    /// A map of the disk (one block per partition, sized by its share) and a partition table.
    fn partitions_view(&self, ui: &mut egui::Ui, d: &Disk, pal: &Palette) {
        if d.partitions.is_empty() {
            ui.label(RichText::new(self.tr("no_parts")).italics().color(pal.muted));
            return;
        }
        let colors = [
            pal.accent,
            Color32::from_rgb(46, 170, 120),
            Color32::from_rgb(200, 140, 40),
            Color32::from_rgb(150, 100, 200),
            Color32::from_rgb(60, 170, 190),
            Color32::from_rgb(200, 90, 110),
        ];
        // Disk map.
        let free = parts::unallocated(d);
        let total = (d.partitions.iter().map(|p| p.size).sum::<u64>() + free).max(1);
        let width = ui.available_width() - 8.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 30.0), egui::Sense::hover());
        let mut x = rect.min.x;
        let mut blocks: Vec<(egui::Rect, Color32, String, String)> = Vec::new();
        for (i, p) in d.partitions.iter().enumerate() {
            let w = (p.size as f32 / total as f32 * width).max(6.0);
            let r = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), Vec2::new(w.min(rect.max.x - x), 30.0));
            let title = if p.mounts.is_empty() { p.name.clone() } else { format!("{} {}", p.name, p.mounts[0]) };
            let tip = format!(
                "{}\n{} · {}\n{}",
                p.name,
                self.bytes_text(p.size),
                if p.fs.is_empty() { "—" } else { &p.fs },
                p.mounts.join("\n")
            );
            blocks.push((r, colors[i % colors.len()], title, tip));
            x += w;
        }
        if free > total / 200 && x < rect.max.x {
            let r = egui::Rect::from_min_max(egui::pos2(x, rect.min.y), rect.max);
            blocks.push((r, pal.unknown.gamma_multiply(0.5), self.tr("d_unallocated").to_string(), self.bytes_text(free)));
        }
        let painter = ui.painter_at(rect);
        for (r, color, title, tip) in &blocks {
            let inner = r.shrink(1.0);
            painter.rect_filled(inner, 3.0, *color);
            let galley = painter.layout_no_wrap(title.clone(), egui::FontId::proportional(11.0), Color32::WHITE);
            if galley.size().x + 6.0 < inner.width() {
                painter.galley(inner.left_center() + Vec2::new(4.0, -galley.size().y / 2.0), galley, Color32::WHITE);
            }
            ui.interact(*r, ui.id().with(("block", r.min.x as i32)), egui::Sense::hover()).on_hover_text(tip);
        }
        ui.add_space(6.0);

        // Table.
        egui::Grid::new("parts").striped(true).num_columns(9).spacing([7.0, 2.0]).min_row_height(ROW).show(ui, |ui| {
            ui.label("");
            for key in ["col_part", "col_label", "col_fs", "col_size", "col_usage", "col_free", "col_mount", "col_type"] {
                ui.label(RichText::new(self.tr(key)).strong());
            }
            ui.end_row();
            for (i, p) in d.partitions.iter().enumerate() {
                let (r, _) = ui.allocate_exact_size(Vec2::new(14.0, ROW), egui::Sense::hover());
                ui.painter().rect_filled(egui::Rect::from_center_size(r.center(), Vec2::splat(10.0)), 2.0, colors[i % colors.len()]);
                ui.monospace(&p.name);
                ui.label(if p.label.is_empty() { "—" } else { &p.label });
                ui.label(if p.fs.is_empty() { "—" } else { &p.fs });
                ui.monospace(self.bytes_text(p.size));
                match p.used_fraction() {
                    Some(f) => {
                        // Usage bar: red above 90 %, yellow above 75 %.
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(76.0, 14.0), egui::Sense::hover());
                        let painter = ui.painter();
                        painter.rect_filled(r, 3.0, pal.stripe);
                        painter.rect_stroke(r, 3.0, egui::Stroke::new(1.0_f32, pal.stroke), egui::StrokeKind::Inside);
                        let fill = if f > 0.9 { pal.bad } else if f > 0.75 { pal.caution } else { pal.accent };
                        let filled = egui::Rect::from_min_size(r.min, Vec2::new(r.width() * f.clamp(0.0, 1.0), r.height()));
                        painter.rect_filled(filled.shrink(1.0), 2.0, fill);
                        painter.text(r.center(), egui::Align2::CENTER_CENTER, format!("{:.0} %", f * 100.0), egui::FontId::proportional(10.5), pal.text);
                        resp.on_hover_text(format!(
                            "{}: {}\n{}: {}",
                            self.tr("col_usage"),
                            self.bytes_text(p.used.unwrap_or(0)),
                            self.tr("col_free"),
                            self.bytes_text(p.free.unwrap_or(0))
                        ));
                    }
                    None => {
                        ui.label("—");
                    }
                }
                ui.monospace(p.free.map(|f| self.bytes_text(f)).unwrap_or_else(|| "—".into()));
                // Long paths are shortened here; the whole list shows on hover.
                let short = |s: &str| {
                    let n = s.chars().count();
                    if n > 16 { format!("…{}", s.chars().skip(n - 15).collect::<String>()) } else { s.to_string() }
                };
                let mounts = match p.mounts.len() {
                    0 => "—".to_string(),
                    1 => short(&p.mounts[0]),
                    n => format!("{} (+{})", short(&p.mounts[0]), n - 1),
                };
                let tip = if p.mounts.is_empty() { self.tr("h_mounts").to_string() } else { p.mounts.join("\n") };
                ui.label(mounts).on_hover_text(tip);
                let kind = if p.uuid.is_empty() { p.kind.clone() } else { format!("{}\nUUID {}", p.kind, p.uuid) };
                ui.label(if p.kind.is_empty() { "—" } else { &p.kind }).on_hover_text(kind);
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        let table = d.details.iter().find(|(k, _)| *k == "d_part_table").map(|(_, v)| v.as_str()).unwrap_or("—");
        ui.label(
            RichText::new(format!(
                "{}: {}   ·   {}: {}",
                self.tr("d_part_table"),
                table,
                self.tr("d_unallocated"),
                self.bytes_text(free)
            ))
            .color(pal.muted),
        );
    }

    /// Everything else known about the disk, as name: value rows.
    fn details_view(&self, ui: &mut egui::Ui, d: &Disk) {
        let mut rows: Vec<(String, String)> = vec![
            (self.tr("model").into(), d.model.clone()),
            (self.tr("d_capacity_bytes").into(), format!("{} B", self.group_digits(d.size_bytes))),
        ];
        rows.extend(d.details.iter().map(|(k, v)| (self.tr(k).to_string(), self.value_text(v))));
        if rows.len() <= 2 {
            ui.label(self.tr("no_details"));
        }
        egui::Grid::new("details").striped(true).num_columns(2).spacing([16.0, 2.0]).min_row_height(ROW).show(ui, |ui| {
            for (k, v) in rows {
                ui.label(RichText::new(k).strong());
                ui.add(egui::Label::new(RichText::new(v).monospace()).wrap());
                ui.end_row();
            }
        });
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        ctx.request_repaint();
        shot.frames += 1;
        if shot.frames >= 120 && !shot.requested {
            shot.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(dir) = shot.path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = image::save_buffer(&shot.path, &bytes, w as u32, h as u32, image::ColorType::Rgba8) {
                eprintln!("screenshot not saved: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// The program icon, built into the program.
fn window_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../packaging/diskinfo.png")).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width, height })
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let shot = arg_value(&args, "--screenshot").map(PathBuf::from);
    // --demo shows two sample disks with S.M.A.R.T. data (for checking the layout).
    let demo = args.iter().any(|a| a == "--demo");
    let tab = arg_value(&args, "--tab");
    // Keep the application-menu entry current (it gives the window its icon on Wayland).
    // Checks (--screenshot) and --demo runs leave the user's files alone.
    #[cfg(target_os = "linux")]
    if shot.is_none() && !demo {
        let s = Settings::load();
        if s.menu_entry {
            integrate::install(t(s.language, "about_1"));
        }
    }
    // Optional overrides, handy for checking a language or theme: --lang en --theme dark
    let lang = arg_value(&args, "--lang").and_then(|v| match v.as_str() {
        "es" => Some(Lang::Es),
        "en" => Some(Lang::En),
        "pt" => Some(Lang::Pt),
        "fr" => Some(Lang::Fr),
        "de" => Some(Lang::De),
        "it" => Some(Lang::It),
        _ => None,
    });
    let theme = arg_value(&args, "--theme").and_then(|v| match v.as_str() {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        "mint" => Some(Theme::Mint),
        "ocean" => Some(Theme::Ocean),
        "sakura" => Some(Theme::Sakura),
        "contrast" => Some(Theme::Contrast),
        _ => None,
    });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_inner_size([760.0, 660.0])
        .with_min_inner_size([560.0, 480.0])
        // Matches the .desktop file name, so the desktop shows the same icon in the dock.
        .with_app_id("diskinfo");
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    if shot.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |_cc| {
            let mut app = App::new(shot, lang, theme, demo);
            // --tab parts|details opens on that tab (handy for checking the layout).
            app.tab = match tab.as_deref() {
                Some("parts") => Tab::Partitions,
                Some("details") => Tab::Details,
                _ => Tab::Smart,
            };
            Ok(Box::new(app))
        }),
    )
}
