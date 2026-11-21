// No console window behind the app on Windows (release builds).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod demo;
mod disks;
mod alerts;
mod autostart;
mod bench;
mod bench_ui;
mod drive_picker;
mod fake_ui;
mod fakecheck;
#[cfg(target_os = "linux")]
mod rawcheck;
mod cli;
mod health;
mod history;
mod monitor;
mod parts;
mod i18n;
#[cfg(target_os = "linux")]
mod integrate;
mod platform;
#[cfg(target_os = "linux")]
mod polkit;
mod smartctl;
mod speed;
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
    /// Desktop notifications when a disk gets worse or too hot.
    alerts: bool,
    /// Play an alarm sound when a disk gets worse or too hot.
    alert_sound: bool,
    /// Show serial numbers as asterisks (handy for screenshots).
    hide_serial: bool,
    /// Hide the S.M.A.R.T. attribute table.
    hide_smart: bool,
    /// Attribute names always in English, whatever the interface language.
    smart_english: bool,
    /// Leave out disks that give no S.M.A.R.T. data (when some disk does).
    hide_no_smart: bool,
    /// Size of the interface, in percent.
    zoom: u32,
    /// Start when the person logs in (minimised).
    autostart: bool,
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
            alerts: true,
            alert_sound: true,
            hide_serial: false,
            hide_smart: false,
            smart_english: false,
            hide_no_smart: false,
            zoom: 100,
            autostart: false,
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
    /// Drives as the system lists them right now (for the test windows), refreshed while one is open.
    storage: Vec<Disk>,
    storage_rx: Option<Receiver<Vec<Disk>>>,
    storage_at: Option<Instant>,
    storage_force: bool,
    /// Folders the person picked by hand for the tests, and the folder dialog that is open.
    custom_dirs: Vec<PathBuf>,
    folder_rx: Option<Receiver<Result<Option<PathBuf>, String>>>,
    scan_admin: bool,
    needs_admin: bool,
    direct_ok: bool,
    message: Option<&'static str>,
    show_thresholds: bool,
    edit_th: Thresholds,
    show_about: bool,
    toast: Option<(String, Instant)>,
    /// A picture of the window being saved: waits a moment so the menu has closed.
    save_image: Option<ImageJob>,
    /// `--minimized`: minimise the window on the first frame (started at login).
    minimize_first: bool,
    /// A test window was just opened: make the main window at least this big (points), since
    /// its windows cannot be larger than the window that holds them.
    grow_to: Option<(f32, f32)>,
    /// `--try-bench`: run a small speed test from the window, print it and quit.
    try_bench: bool,
    /// `--try-save`: save the report as text and picture, then quit (to check saving works).
    try_save: Option<u32>,
    applied_theme: Option<Theme>,
    /// The passwordless rule let us read S.M.A.R.T. on the last scan.
    passwordless: bool,
    show_pw: bool,
    /// Installing or removing the rule: (turning on?, result).
    pw_task: Option<Receiver<(bool, Result<(), &'static str>)>>,
    /// Which view is shown under the disk facts.
    tab: Tab,
    shot: Option<Shot>,
    /// Screenshot and demo runs: no history saved, no notifications.
    quiet: bool,
    /// Shared with the background monitor: history, alert log, what it read last.
    core: monitor::Shared,
    show_alerts: bool,
    /// The disk speed test window.
    bench: bench_ui::BenchUi,
    /// The "is this drive real?" window.
    fake: fake_ui::FakeUi,
    speeds: speed::Speeds,
    /// Which line the History tab draws.
    metric: Metric,
    show_selftest: bool,
    /// Starting a self-test: (extended?, result).
    selftest_task: Option<Receiver<(bool, Result<(), &'static str>)>>,
}

pub const APP_NAME: &str = "DiskInfo++";

const AUTO_REFRESH_CHOICES: [u32; 8] = [0, 1, 3, 5, 10, 30, 60, 120];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Smart,
    Partitions,
    History,
    Details,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Metric {
    Temperature,
    Life,
    Written,
    BadSectors,
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
        let settings_for_core = settings.clone();
        let mut app = Self {
            disks: Vec::new(),
            selected: 0,
            settings,
            scanning: None,
            storage: Vec::new(),
            storage_rx: None,
            storage_at: None,
            storage_force: false,
            custom_dirs: Vec::new(),
            folder_rx: None,
            scan_admin: false,
            needs_admin: false,
            direct_ok: false,
            message: None,
            show_thresholds: false,
            edit_th: Thresholds::defaults(false),
            show_about: false,
            toast: None,
            save_image: None,
            try_save: None,
            try_bench: false,
            minimize_first: false,
            grow_to: None,
            applied_theme: None,
            passwordless: false,
            show_pw: false,
            pw_task: None,
            tab: Tab::Smart,
            quiet: demo || shot.is_some(),
            core: {
                let quiet = demo || shot.is_some();
                let (hist, log) = if quiet {
                    (history::History::default(), alerts::Log::default())
                } else {
                    (history::History::load(), alerts::Log::load())
                };
                monitor::new(settings_for_core.clone(), hist, log)
            },
            show_alerts: false,
            bench: bench_ui::BenchUi::default(),
            fake: fake_ui::FakeUi::default(),
            speeds: speed::start(),
            metric: Metric::Temperature,
            show_selftest: false,
            selftest_task: None,
            shot: shot.map(|path| Shot { path, frames: 0, requested: false }),
        };
        if demo {
            app.apply_scan(demo::demo_scan(), false);
            {
                let mut c = monitor::lock(&app.core);
                c.history = demo::demo_history(&app.disks);
                c.log = demo::demo_alerts(&app.disks);
            }
        } else if app.shot.is_some() {
            // Read synchronously so the picture shows real disks.
            let r = disks::scan(false);
            app.apply_scan(r, false);
        } else {
            app.start_scan(false);
        }
        if !app.quiet {
            // Keeps watching (history, alerts) even when the window is minimised or hidden.
            monitor::start(app.core.clone());
        }
        app
    }

    fn tr(&self, key: &'static str) -> &'static str {
        t(self.settings.language, key)
    }

    fn pal(&self) -> Palette {
        self.settings.theme.palette()
    }

    /// The drives the test windows list: the fresh listing when there is one.
    pub(crate) fn pick_disks(&self) -> &Vec<Disk> {
        if self.storage.is_empty() {
            &self.disks
        } else {
            &self.storage
        }
    }

    /// While a test window is open, keeps the list of drives and mounts up to date (a USB stick
    /// plugged in or mounted from outside shows up without closing the program).
    fn tick_storage(&mut self, ctx: &egui::Context) {
        if self.quiet || !(self.bench.open || self.fake.open) {
            return;
        }
        // "Choose folder…": the system's folder dialog runs in its own thread.
        let req = egui::Id::new("pick_folder_req");
        if ctx.data(|d| d.get_temp::<bool>(req)).unwrap_or(false) {
            ctx.data_mut(|d| d.remove::<bool>(req));
            if self.folder_rx.is_none() {
                let (tx, rx) = channel();
                let title = self.tr("pick_folder_title").to_string();
                std::thread::spawn(move || {
                    let _ = tx.send(platform::pick_folder(&title));
                });
                self.folder_rx = Some(rx);
            }
        }
        if let Some(rx) = &self.folder_rx {
            match rx.try_recv() {
                Ok(r) => {
                    self.folder_rx = None;
                    match r {
                        Ok(Some(p)) if p.is_dir() => {
                            if !self.custom_dirs.contains(&p) {
                                self.custom_dirs.push(p.clone());
                            }
                            self.rebuild_test_dirs();
                            if let Some(i) = self.bench.dirs.iter().position(|d| d.0 == p) {
                                self.bench.dir_idx = i;
                            }
                            if let Some(i) = self.fake.dirs.iter().position(|d| d.0 == p) {
                                self.fake.dir_idx = i;
                            }
                        }
                        Err(_) => {
                            let msg = self.tr("pick_folder_none").to_string();
                            ctx.data_mut(|d| d.insert_temp(egui::Id::new("mount_msg"), msg));
                        }
                        _ => {}
                    }
                }
                Err(_) => ctx.request_repaint_after(Duration::from_millis(300)),
            }
        }
        if let Some(rx) = &self.storage_rx {
            if let Ok(list) = rx.try_recv() {
                self.storage_rx = None;
                self.storage_at = Some(Instant::now());
                if !list.is_empty() {
                    let sig = |l: &Vec<Disk>| -> String {
                        l.iter().map(|d| format!("{}:{}:{:?}", d.dev, d.size_bytes, d.partitions.iter().map(|p| (&p.name, &p.mounts, &p.fs)).collect::<Vec<_>>())).collect()
                    };
                    let changed = sig(&list) != sig(&self.storage);
                    self.storage = list;
                    if changed {
                        self.rebuild_test_dirs();
                    }
                }
            }
            ctx.request_repaint_after(Duration::from_millis(250));
            return;
        }
        let busy = self.bench.handle.is_some() || self.fake.handle.is_some();
        let periodic = cfg!(target_os = "linux") && !busy;
        let due = self.storage_force || self.storage_at.is_none() || (periodic && self.storage_at.is_some_and(|t| t.elapsed() > Duration::from_secs(3)));
        if due {
            self.storage_force = false;
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(disks::storage_snapshot());
            });
            self.storage_rx = Some(rx);
        }
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    /// Rebuilds the folder lists of the test windows, keeping what was chosen when it still exists.
    pub(crate) fn rebuild_test_dirs(&mut self) {
        let dirs = self.bench_folders();
        let keep = |old: &Vec<(PathBuf, String)>, idx: usize| old.get(idx).and_then(|d| dirs.iter().position(|n| n.0 == d.0));
        let b_idx = keep(&self.bench.dirs, self.bench.dir_idx);
        let f_idx = keep(&self.fake.dirs, self.fake.dir_idx);
        let after = self.fake.after_dev.take();
        self.bench.dirs = dirs.clone();
        self.bench.dir_idx = b_idx.unwrap_or(0);
        // After a test the chosen partition may be gone (unmounted): fall back to its whole drive.
        let fallback = after.and_then(|dev| dirs.iter().position(|n| n.0 == PathBuf::from(format!("/dev/{dev}"))));
        self.fake.dir_idx = f_idx.or(fallback).unwrap_or(0);
        self.fake.dirs = dirs;
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
        // Leave out disks without S.M.A.R.T. data, but only when at least one disk has it.
        if self.settings.hide_no_smart && self.disks.iter().any(|d| d.smart_read) {
            self.disks.retain(|d| d.smart_read);
        }
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
        self.after_reading();
    }

    /// After every reading: hand the disks to the background monitor (it keeps the history and
    /// announces disks that got worse or too hot). Checks and demo runs (`quiet`) are left out.
    fn after_reading(&mut self) {
        if self.quiet {
            return;
        }
        let mut c = monitor::lock(&self.core);
        c.snapshot = self.disks.clone();
        c.direct_ok = self.direct_ok;
        c.observe(&self.disks);
    }

    fn reevaluate(&mut self) {
        for d in &mut self.disks {
            let th = self.settings.thresholds_for(d);
            health::evaluate(d, &th);
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

    /// Why the health is not Good, in words ("05 Reallocated sectors: 8 (limit 1)").
    fn health_reason(&self, d: &Disk) -> String {
        if d.health == Status::Good || d.health == Status::Unknown {
            return String::new();
        }
        let th = self.settings.thresholds_for(d);
        d.health_why
            .iter()
            .map(|(kind, id, v)| {
                let name = d.attrs.iter().find(|a| a.id == *id).map(|a| self.attr_name(d, a)).unwrap_or_default();
                let limit = match (kind.as_str(), *id) {
                    ("count", 0x05) => Some(th.t05 as i64),
                    ("count", 0xC5) => Some(th.tc5 as i64),
                    ("count", 0xC6) => Some(th.tc6 as i64),
                    ("life", _) => Some(th.tff as i64),
                    _ => None,
                };
                let rule = self.tr(match kind.as_str() {
                    "count" => "why_count",
                    "thresh" => "why_thresh",
                    "life" => "why_life",
                    "spare" => "why_spare",
                    _ => "why_critical",
                });
                match limit {
                    Some(l) => format!("{id:02X} {name}: {v} ({rule} {l})"),
                    None => format!("{id:02X} {name}: {v} ({rule})"),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
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
        let lang = if self.settings.smart_english { Lang::En } else { self.settings.language };
        i18n::attr_name(lang, a.id, d.is_nvme, &a.name)
    }

    /// The serial number, or asterisks when the person chose to hide it.
    fn serial_text(&self, d: &Disk) -> String {
        if self.settings.hide_serial && !d.serial.is_empty() {
            "*".repeat(d.serial.chars().count().clamp(6, 16))
        } else {
            d.serial.clone()
        }
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
                (self.tr("serial"), self.serial_text(d)),
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
            if !d.attrs.is_empty() && !self.settings.hide_smart {
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
        if self.minimize_first {
            self.minimize_first = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        let zoom = self.settings.zoom.clamp(70, 250) as f32 / 100.0;
        if (ctx.zoom_factor() - zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
        }
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
        // Hand the settings to the background monitor and pick up what it read.
        let pending = if self.quiet {
            None
        } else {
            let mut c = monitor::lock(&self.core);
            c.settings = self.settings.clone();
            c.pending.take()
        };
        match pending {
            Some(monitor::Pending::Scan(r)) if self.scanning.is_none() => self.apply_scan(r, false),
            Some(monitor::Pending::Temps(temps)) => {
                for (dev, temp) in temps {
                    if let Some(d) = self.disks.iter_mut().find(|d| d.dev == dev) {
                        d.temperature = Some(temp);
                    }
                }
                if !self.quiet {
                    monitor::lock(&self.core).snapshot = self.disks.clone();
                }
            }
            _ => {}
        }
        // Once a second: live speed, and alerts keep working while the window is in the background.
        ctx.request_repaint_after(Duration::from_secs(1));
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.refresh();
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button(self.tr("menu_file"), |ui| {
                    let label = self.tr("autostart_menu");
                    if ui.checkbox(&mut self.settings.autostart, label).changed() {
                        if autostart::set(self.settings.autostart) {
                            self.settings.save();
                        } else {
                            self.settings.autostart = !self.settings.autostart;
                            self.toast_key("save_failed");
                        }
                    }
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
                        self.toast_key("report_copied");
                        ui.close_menu();
                    }
                    if ui.button(self.tr("save_text")).clicked() {
                        self.save_report_text();
                        ui.close_menu();
                    }
                    if ui.button(self.tr("save_image")).clicked() {
                        self.start_save_image();
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
                    if ui.button(self.tr("fake_menu")).clicked() {
                        self.fake_open();
                        ui.close_menu();
                    }
                    if ui.button(self.tr("bench_menu")).clicked() {
                        self.bench_open();
                        ui.close_menu();
                    }
                    if ui.button(self.tr("selftest_menu")).clicked() {
                        self.show_selftest = true;
                        ui.close_menu();
                    }
                    let label = self.tr("alerts_menu");
                    if ui.checkbox(&mut self.settings.alerts, label).changed() {
                        self.settings.save();
                    }
                    let label = self.tr("alert_sound_menu");
                    if ui.checkbox(&mut self.settings.alert_sound, label).changed() {
                        self.settings.save();
                    }
                    if ui.button(self.tr("alert_sound_test")).clicked() {
                        alerts::play_sound();
                        ui.close_menu();
                    }
                    if ui.button(self.tr("alert_history_menu")).clicked() {
                        self.show_alerts = true;
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
                ui.menu_button(self.tr("menu_view"), |ui| {
                    ui.menu_button(self.tr("v_zoom"), |ui| {
                        for z in [80u32, 90, 100, 110, 125, 150, 175, 200] {
                            if ui.radio(self.settings.zoom == z, format!("{z} %")).clicked() {
                                self.settings.zoom = z;
                                self.settings.save();
                                ui.close_menu();
                            }
                        }
                    });
                    ui.separator();
                    let labels = [self.tr("v_hide_serial"), self.tr("v_hide_smart"), self.tr("v_smart_english"), self.tr("v_hide_nosmart")];
                    let mut changed = false;
                    changed |= ui.checkbox(&mut self.settings.hide_serial, labels[0]).changed();
                    changed |= ui.checkbox(&mut self.settings.hide_smart, labels[1]).changed();
                    changed |= ui.checkbox(&mut self.settings.smart_english, labels[2]).changed();
                    if ui.checkbox(&mut self.settings.hide_no_smart, labels[3]).changed() {
                        changed = true;
                        self.refresh();
                    }
                    if changed {
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
                // Live activity, refreshed every second.
                let sp = self.speeds.lock().ok().and_then(|m| m.get(&speed::key_for(&d)).copied());
                let rate = |b: Option<f64>| match b {
                    Some(v) => format!("{}/s", self.bytes_text(v as u64)),
                    None => "—".into(),
                };
                let line = match sp {
                    Some(s) if s.total.is_some() => format!("{}: {}", self.tr("activity"), rate(s.total)),
                    Some(s) => format!(
                        "{}:  {} {}   ·   {} {}",
                        self.tr("activity"),
                        self.tr("act_read"),
                        rate(s.read),
                        self.tr("act_write"),
                        rate(s.write)
                    ),
                    None => format!("{}: —", self.tr("activity")),
                };
                ui.label(RichText::new(line).color(pal.muted)).on_hover_text(self.tr("h_activity"));
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
                    let why = self.health_reason(&d);
                    let health_tip = if why.is_empty() {
                        format!("{}\n\n{}", self.health_text(&d), self.tr("h_health"))
                    } else {
                        format!("{}\n{}: {}\n\n{}", self.health_text(&d), self.tr("why_title"), why, self.tr("h_health"))
                    };
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
                        (f("serial", self.serial_text(&d), "h_serial"), f("host_writes", opt(d.host_writes_gb, " GB"), "h_host_writes")),
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
                for (tab, key) in [
                    (Tab::Smart, "tab_smart"),
                    (Tab::Partitions, "tab_parts"),
                    (Tab::History, "tab_history"),
                    (Tab::Details, "tab_details"),
                ] {
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
                        Tab::History => self.history_view(ui, &d, &pal),
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
                        self.toast_key(if enable { "msg_rule_on" } else { "msg_rule_off" });
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

        // Result of starting a self-test.
        if let Some(rx) = &self.selftest_task {
            if let Ok((_, res)) = rx.try_recv() {
                self.selftest_task = None;
                match res {
                    Ok(()) => {
                        self.toast_key("msg_test_started");
                        self.message = None;
                        self.refresh();
                    }
                    Err(e) => self.message = Some(e),
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }

        if self.show_selftest && !self.disks.is_empty() {
            let lang = self.settings.language;
            let d = self.disks[self.selected.min(self.disks.len() - 1)].clone();
            let busy = self.selftest_task.is_some();
            let mut start: Option<bool> = None;
            let mut refresh = false;
            let pal = self.pal();
            egui::Window::new(t(lang, "selftest_menu").trim_end_matches('…'))
                .open(&mut self.show_selftest)
                .resizable(false)
                .collapsible(false)
                .default_width(480.0)
                .show(ctx, |ui| {
                    ui.label(RichText::new(format!("{} ({})", d.model, d.place())).strong());
                    ui.label(t(lang, "st_explain"));
                    ui.add_space(6.0);
                    let st = &d.selftest;
                    if !d.smart_read {
                        ui.label(RichText::new(t(lang, "st_need_read")).color(pal.error));
                    } else if !st.supported {
                        ui.label(RichText::new(t(lang, "st_unsupported")).color(pal.muted));
                    }
                    if let Some(done) = st.running {
                        ui.add(egui::ProgressBar::new(done as f32 / 100.0).text(format!("{} {done} %", t(lang, "st_running"))));
                    }
                    if !st.status.is_empty() {
                        ui.label(format!("{}: {}", t(lang, "st_now"), st.status));
                    }
                    let mins = |m: Option<u64>| m.map(|m| format!(" (~{m} min)")).unwrap_or_default();
                    ui.horizontal(|ui| {
                        let can = !busy && st.running.is_none() && d.smart_read;
                        if ui.add_enabled(can, egui::Button::new(format!("{}{}", t(lang, "st_short"), mins(st.short_min)))).clicked() {
                            start = Some(false);
                        }
                        if ui.add_enabled(can, egui::Button::new(format!("{}{}", t(lang, "st_long"), mins(st.long_min)))).clicked() {
                            start = Some(true);
                        }
                        if ui.button(t(lang, "st_check")).clicked() {
                            refresh = true;
                        }
                        if busy {
                            ui.spinner();
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new(t(lang, "st_log")).strong());
                    if st.log.is_empty() {
                        ui.label(RichText::new(t(lang, "st_log_empty")).italics().color(pal.muted));
                    } else {
                        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                            egui::Grid::new("selftest_log").striped(true).num_columns(3).spacing([12.0, 2.0]).show(ui, |ui| {
                                ui.label(RichText::new(t(lang, "col_type")).strong());
                                ui.label(RichText::new(t(lang, "st_result")).strong());
                                ui.label(RichText::new(t(lang, "power_on_hours")).strong());
                                ui.end_row();
                                for (kind, result, hours) in &st.log {
                                    ui.label(kind);
                                    let bad = !(result.contains("without error") || result.contains("Completed: no error") || result.is_empty());
                                    ui.label(RichText::new(result).color(if bad { pal.bad } else { pal.text }));
                                    ui.label(hours.map(|h| h.to_string()).unwrap_or_default());
                                    ui.end_row();
                                }
                            });
                        });
                    }
                });
            if refresh {
                self.refresh();
            }
            if let Some(extended) = start {
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let _ = tx.send((extended, disks::start_selftest(&d, extended)));
                });
                self.selftest_task = Some(rx);
            }
        }

        if self.show_alerts {
            let lang = self.settings.language;
            let events = monitor::lock(&self.core).log.events.clone();
            let mut clear = false;
            let pal = self.pal();
            // Text of each event, worked out before the window so it does not borrow `self`.
            let rows: Vec<(String, String, String, Color32)> = events
                .iter()
                .rev()
                .map(|e| {
                    let (text, color) = match e.kind.as_str() {
                        "health" => (
                            format!("{}: {}", t(lang, "health_status"), t_dyn_status(lang, &e.value)),
                            if e.value == "bad" { pal.bad } else { pal.caution },
                        ),
                        _ => (format!("{}: {}", t(lang, "alert_hot_short"), self.temp_text(e.value.parse().ok())), pal.bad),
                    };
                    (date_time_text(e.t), e.disk.clone(), text, color)
                })
                .collect();
            let mut open = true;
            egui::Window::new(t(lang, "alert_history_title"))
                .open(&mut open)
                .resizable(true)
                .collapsible(false)
                .default_width(520.0)
                .show(ctx, |ui| {
                    if rows.is_empty() {
                        ui.label(RichText::new(t(lang, "alert_history_empty")).italics().color(pal.muted));
                    } else {
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            egui::Grid::new("alert_log").striped(true).num_columns(3).spacing([14.0, 3.0]).show(ui, |ui| {
                                for (when, disk, text, color) in &rows {
                                    ui.label(when);
                                    ui.label(disk);
                                    ui.label(RichText::new(text).color(*color));
                                    ui.end_row();
                                }
                            });
                        });
                        ui.add_space(6.0);
                        if ui.button(t(lang, "alert_history_clear")).clicked() {
                            clear = true;
                        }
                    }
                });
            self.show_alerts = open;
            if clear {
                let mut c = monitor::lock(&self.core);
                c.log.events.clear();
                c.log.save();
            }
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

        if let Some((text, at)) = self.toast.clone().filter(|_| self.save_image.is_none()) {
            if at.elapsed() < Duration::from_secs(if text.contains('/') || text.contains('\\') { 6 } else { 3 }) {
                egui::Area::new("toast".into()).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -16.0]).show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(text.as_str()));
                });
                ctx.request_repaint_after(Duration::from_millis(200));
            } else {
                self.toast = None;
            }
        }

        if let Some((w, h)) = self.grow_to.take() {
            let cur = ctx.screen_rect().size();
            if cur.x < w || cur.y < h {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(cur.x.max(w), cur.y.max(h))));
            }
        }
        self.tick_storage(ctx);
        self.bench_window(ctx);
        self.fake_window(ctx);
        if self.try_bench {
            ctx.request_repaint();
            if let Some(last) = self.bench.last.as_ref().filter(|s| !s.running()) {
                print!("{:?}\n{}", last.phase, bench::format_table(&last.results, "Read", "Write"));
                self.try_bench = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        self.save_image_step(ctx);
        if let Some(n) = self.try_save {
            self.try_save = Some(n + 1);
            ctx.request_repaint();
            if n == 20 {
                self.save_report_text();
                self.start_save_image();
            } else if n > 20 && self.save_image.is_none() {
                println!("{}", self.toast.as_ref().map(|t| t.0.as_str()).unwrap_or("no toast"));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
        if self.settings.hide_smart {
            ui.label(RichText::new(self.tr("smart_hidden")).italics().color(pal.muted));
            return;
        }
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
        egui::Grid::new("parts").striped(true).num_columns(10).spacing([7.0, 2.0]).min_row_height(ROW).show(ui, |ui| {
            ui.label("");
            for key in ["col_part", "col_label", "col_fs", "col_size", "col_usage", "col_free", "col_mount", "col_state", "col_type"] {
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
                // State: encryption, read-only, inodes, the system's verdict.
                let (state, warn) = self.partition_state(p);
                let color = if warn { pal.bad } else { pal.text };
                ui.label(RichText::new(state).color(color)).on_hover_text(self.tr("h_part_state"));
                ui.label(if p.kind.is_empty() { "—" } else { &p.kind }).on_hover_text(kind);
                ui.end_row();

                // What lives inside: subvolumes, datasets, encrypted/LVM/RAID devices.
                for sv in &p.subs {
                    ui.label("");
                    ui.label(RichText::new(format!("   · {}", sv.name)).monospace().color(pal.muted))
                        .on_hover_text(if sv.id.is_empty() { sv.name.clone() } else { format!("{}\nID {}", sv.name, sv.id) });
                    ui.label(RichText::new(self.tr(sv.kind)).color(pal.muted));
                    ui.label(if sv.fs.is_empty() { "—" } else { &sv.fs });
                    ui.monospace(sv.size.map(|b| self.bytes_text(b)).unwrap_or_else(|| "—".into()))
                        .on_hover_text(if sv.size.is_none() { self.tr("h_sub_shared") } else { "" });
                    match (sv.used, sv.free) {
                        (Some(u), Some(f)) if u + f > 0 => {
                            ui.label(format!("{:.0} %", u as f64 * 100.0 / (u + f) as f64));
                        }
                        _ => {
                            ui.label("—");
                        }
                    }
                    ui.monospace(sv.free.map(|f| self.bytes_text(f)).unwrap_or_else(|| "—".into()));
                    let mounts = match sv.mounts.len() {
                        0 => "—".to_string(),
                        1 => short(&sv.mounts[0]),
                        n => format!("{} (+{})", short(&sv.mounts[0]), n - 1),
                    };
                    let tip = if sv.mounts.is_empty() { self.tr("h_sub_unmounted").to_string() } else { sv.mounts.join("\n") };
                    ui.label(mounts).on_hover_text(tip);
                    ui.label("");
                    ui.label("");
                    ui.end_row();
                }
            }
        });
        if d.partitions.iter().any(|p| p.subs.iter().any(|s| s.kind == "sub_btrfs")) {
            ui.label(RichText::new(self.tr("h_btrfs_note")).small().color(pal.muted));
        }
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
        let grid = |ui: &mut egui::Ui, id: &str, rows: Vec<(String, String)>| {
            egui::Grid::new(id).striped(true).num_columns(2).spacing([16.0, 2.0]).min_row_height(ROW).show(ui, |ui| {
                for (k, v) in rows {
                    ui.label(RichText::new(k).strong());
                    ui.add(egui::Label::new(RichText::new(v).monospace()).wrap());
                    ui.end_row();
                }
            });
        };
        grid(ui, "details", rows);

        // SSD life estimate.
        if let Some(e) = history::estimate(d, &self.samples_of(d)) {
            ui.add_space(8.0);
            ui.label(RichText::new(self.tr("est_title")).strong().size(15.0)).on_hover_text(self.tr("h_estimate"));
            let tb = |v: f64| self.decimal(format!("{v:.1} TB"));
            let mut est = vec![(self.tr("est_written").to_string(), tb(e.written_tb))];
            if let (Some(total), Some(rem)) = (e.total_tb, e.remaining_tb) {
                est.push((self.tr("est_total").into(), tb(total)));
                est.push((self.tr("est_remaining").into(), tb(rem)));
            } else {
                est.push((self.tr("est_total").into(), self.tr("est_no_wear").into()));
            }
            match (e.gb_per_day, e.days_left) {
                (Some(per_day), Some(days)) => {
                    est.push((self.tr("est_pace").into(), self.decimal(format!("{per_day:.1} GB/{}", self.tr("unit_day")))));
                    let years = days / 365.25;
                    let when = if years > 50.0 {
                        self.tr("est_more_50").to_string()
                    } else {
                        let year = 1970.0 + (history::now() as f64 / 86_400.0 + days) / 365.25;
                        format!("{} ({} {:.0})", self.decimal(format!("{years:.1}")), self.tr("est_around"), year.floor())
                    };
                    est.push((self.tr("est_years_left").into(), when));
                }
                _ => est.push((self.tr("est_years_left").into(), self.tr("est_need_history").into())),
            }
            grid(ui, "estimate", est);
        }

        // Facts the disk names itself, by group.
        for group in ["x_temps", "x_power", "x_errors", "x_phy", "x_devstat"] {
            let rows: Vec<(String, String)> = d
                .extra
                .iter()
                .filter(|(g, _, _)| *g == group)
                .map(|(_, n, v)| (self.extra_name(n), self.value_text(v)))
                .collect();
            if rows.is_empty() {
                continue;
            }
            ui.add_space(8.0);
            ui.label(RichText::new(self.tr(group)).strong().size(15.0));
            grid(ui, group, rows);
        }
    }

    /// The recorded readings of a disk (a copy, so the monitor thread is not kept waiting).
    fn samples_of(&self, d: &Disk) -> Vec<history::Sample> {
        monitor::lock(&self.core).history.samples(d).to_vec()
    }

    /// "x_sensor:2" → "Sensor 2"; other "x_" names are text keys; anything else is shown as is.
    fn extra_name(&self, n: &str) -> String {
        let (key, rest) = n.split_once(':').unwrap_or((n, ""));
        let known = i18n::has(key);
        match (known, rest.is_empty()) {
            (true, true) => i18n::t_dyn(self.settings.language, key).to_string(),
            (true, false) => format!("{} {rest}", i18n::t_dyn(self.settings.language, key)),
            _ => n.to_string(),
        }
    }

    /// "12.5" → "12,5" in languages with a decimal comma.
    fn decimal(&self, s: String) -> String {
        if self.settings.language.decimal_comma() {
            s.replace('.', ",")
        } else {
            s
        }
    }

    /// Short state of a partition and whether it needs attention.
    fn partition_state(&self, p: &parts::Partition) -> (String, bool) {
        let mut items = Vec::new();
        let mut warn = false;
        if !p.encryption.is_empty() {
            items.push(format!("{} ({})", self.tr("st_encrypted"), p.encryption));
        }
        if p.read_only {
            items.push(self.tr("st_read_only").to_string());
        }
        if let Some(f) = p.inodes_used.filter(|f| *f >= 0.8) {
            items.push(format!("{} {:.0} %", self.tr("st_inodes"), f * 100.0));
            warn = true;
        }
        if p.used_fraction().is_some_and(|f| f > 0.95) {
            items.push(self.tr("st_almost_full").to_string());
            warn = true;
        }
        if !p.health.is_empty() && p.health != "Healthy" && p.health != "0" {
            items.push(p.health.clone());
            warn = true;
        }
        if items.is_empty() {
            (if p.mounts.is_empty() { "—".into() } else { "OK".into() }, false)
        } else {
            (items.join(" · "), warn)
        }
    }

    /// A line graph of the chosen measure over the recorded history.
    fn history_view(&mut self, ui: &mut egui::Ui, d: &Disk, pal: &Palette) {
        ui.horizontal(|ui| {
            for (m, key) in [
                (Metric::Temperature, "temperature"),
                (Metric::Life, "m_life"),
                (Metric::Written, "host_writes"),
                (Metric::BadSectors, "m_bad"),
            ] {
                if ui.selectable_label(self.metric == m, self.tr(key)).clicked() {
                    self.metric = m;
                }
            }
        });
        let samples = self.samples_of(d);
        let points: Vec<(u64, f64)> = samples
            .iter()
            .filter_map(|s| {
                let v = match self.metric {
                    Metric::Temperature => s.temp.map(|t| t as f64),
                    Metric::Life => s.life.map(|l| l as f64),
                    Metric::Written => s.written_gb.map(|w| w as f64),
                    Metric::BadSectors => match (s.reallocated, s.pending) {
                        (None, None) => None,
                        (r, p) => Some((r.unwrap_or(0) + p.unwrap_or(0)) as f64),
                    },
                }?;
                Some((s.t, v))
            })
            .collect();
        if points.len() < 2 {
            ui.add_space(6.0);
            ui.label(RichText::new(self.tr("hist_empty")).italics().color(pal.muted));
            return;
        }
        let (t0, t1) = (points[0].0, points[points.len() - 1].0.max(points[0].0 + 1));
        let (mut lo, mut hi) = points.iter().fold((f64::MAX, f64::MIN), |(a, b), (_, v)| (a.min(*v), b.max(*v)));
        if self.metric == Metric::Life {
            hi = 100.0;
            lo = lo.min(90.0).max(0.0);
        }
        if (hi - lo).abs() < 1.0 {
            hi += 1.0;
            lo -= 1.0;
        }
        let pad = (hi - lo) * 0.08;
        let (lo, hi) = (lo - pad, hi + pad);

        let size = Vec2::new(ui.available_width() - 8.0, (ui.available_height() - 30.0).max(160.0));
        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
        let plot = egui::Rect::from_min_max(rect.min + Vec2::new(48.0, 6.0), rect.max - Vec2::new(8.0, 22.0));
        let painter = ui.painter_at(rect);
        let x_of = |t: u64| plot.min.x + (t - t0) as f32 / (t1 - t0) as f32 * plot.width();
        let y_of = |v: f64| plot.max.y - ((v - lo) / (hi - lo)) as f32 * plot.height();
        let font = egui::FontId::proportional(10.5);
        // Grid with values on the left and dates underneath.
        for i in 0..=4 {
            let v = lo + (hi - lo) * i as f64 / 4.0;
            let y = y_of(v);
            painter.line_segment([egui::pos2(plot.min.x, y), egui::pos2(plot.max.x, y)], egui::Stroke::new(1.0_f32, pal.stripe));
            painter.text(egui::pos2(plot.min.x - 4.0, y), egui::Align2::RIGHT_CENTER, format!("{v:.0}"), font.clone(), pal.muted);
        }
        for i in 0..=4 {
            let t = t0 + (t1 - t0) * i / 4;
            let x = x_of(t);
            painter.line_segment([egui::pos2(x, plot.min.y), egui::pos2(x, plot.max.y)], egui::Stroke::new(1.0_f32, pal.stripe));
            let align = match i {
                0 => egui::Align2::LEFT_TOP,
                4 => egui::Align2::RIGHT_TOP,
                _ => egui::Align2::CENTER_TOP,
            };
            painter.text(egui::pos2(x, plot.max.y + 4.0), align, date_text(t), font.clone(), pal.muted);
        }
        painter.rect_stroke(plot, 0.0, egui::Stroke::new(1.0_f32, pal.stroke), egui::StrokeKind::Inside);
        // Alarm line for temperature.
        if self.metric == Metric::Temperature {
            let alarm = self.settings.thresholds_for(d).alarm_temp as f64;
            if alarm > lo && alarm < hi {
                let y = y_of(alarm);
                painter.line_segment([egui::pos2(plot.min.x, y), egui::pos2(plot.max.x, y)], egui::Stroke::new(1.0_f32, pal.bad));
            }
        }
        let line: Vec<egui::Pos2> = points.iter().map(|(t, v)| egui::pos2(x_of(*t), y_of(*v))).collect();
        painter.add(egui::Shape::line(line.clone(), egui::Stroke::new(2.0_f32, pal.accent)));
        // Value under the pointer.
        if let Some(pos) = resp.hover_pos() {
            if let Some((i, p)) = line.iter().enumerate().min_by(|a, b| (a.1.x - pos.x).abs().total_cmp(&(b.1.x - pos.x).abs())) {
                painter.circle_filled(*p, 4.0, pal.accent);
                let (t, v) = points[i];
                let unit = match self.metric {
                    Metric::Temperature => " °C",
                    Metric::Life => " %",
                    Metric::Written => " GB",
                    Metric::BadSectors => "",
                };
                resp.on_hover_text(format!("{}\n{v:.0}{unit}", date_time_text(t)));
            }
        }
        ui.label(
            RichText::new(format!("{} {} · {}", points.len(), self.tr("hist_points"), self.tr("hist_note"))).color(pal.muted),
        );
    }

    fn toast_key(&mut self, key: &'static str) {
        self.toast = Some((self.tr(key).to_string(), Instant::now()));
    }

    /// File name like DiskInfo-2026-11-20-1935.<ext> in the person's Documents folder.
    fn report_path(&self, ext: &str) -> Option<PathBuf> {
        let (y, m, d, h, min) = civil(history::now());
        Some(platform::documents_dir()?.join(format!("DiskInfo-{y}-{m:02}-{d:02}-{h:02}{min:02}.{ext}")))
    }

    fn save_report_text(&mut self) {
        let text = self.report();
        match self.report_path("txt") {
            Some(path) if std::fs::write(&path, text).is_ok() => {
                self.toast = Some((format!("{} {}", self.tr("saved"), path.display()), Instant::now()));
            }
            _ => self.toast_key("save_failed"),
        }
    }

    fn start_save_image(&mut self) {
        if let Some(path) = self.report_path("png") {
            self.toast = None;
            self.save_image = Some(ImageJob { path, at: Instant::now(), requested: false });
        } else {
            self.toast_key("save_failed");
        }
    }

    /// Takes the window picture once the menu has closed, and saves it.
    fn save_image_step(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.save_image else { return };
        if !job.requested {
            if job.at.elapsed() < Duration::from_millis(300) {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            job.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let path = job.path.clone();
            self.save_image = None;
            match save_png(&path, &img) {
                Ok(()) => self.toast = Some((format!("{} {}", self.tr("saved"), path.display()), Instant::now())),
                Err(_) => self.toast_key("save_failed"),
            }
        } else {
            ctx.request_repaint();
        }
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
            if let Some(dir) = shot.path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = save_png(&shot.path, &img) {
                eprintln!("screenshot not saved: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// "caution" → "Precaución" and so on, in the interface language.
fn t_dyn_status(lang: Lang, key: &str) -> &'static str {
    i18n::t_dyn(lang, key)
}

/// A window picture being saved (see `App::save_image_step`).
struct ImageJob {
    path: PathBuf,
    at: Instant,
    requested: bool,
}

fn save_png(path: &std::path::Path, img: &egui::ColorImage) -> Result<(), String> {
    let [w, h] = img.size;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    image::save_buffer(path, &bytes, w as u32, h as u32, image::ColorType::Rgba8).map_err(|e| e.to_string())
}

/// (year, month, day, hour, minute) in UTC from seconds since 1970 (civil-from-days algorithm).
fn civil(t: u64) -> (i64, u32, u32, u32, u32) {
    let days = (t / 86_400) as i64;
    let secs = t % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day, (secs / 3600) as u32, ((secs % 3600) / 60) as u32)
}

/// "19/11" for graph labels.
fn date_text(t: u64) -> String {
    let (_, m, d, _, _) = civil(t);
    format!("{d:02}/{m:02}")
}

/// "19/11/2026 14:05 UTC" for the value under the pointer.
fn date_time_text(t: u64) -> String {
    let (y, m, d, h, min) = civil(t);
    format!("{d:02}/{m:02}/{y} {h:02}:{min:02} UTC")
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
    // `diskinfo --try-sound` plays the alarm once and says whether it worked.
    if args.iter().any(|a| a == "--try-sound") {
        let ok = alerts::play_sound_blocking();
        println!("{}", if ok { "sound ok" } else { "sound failed: no player found" });
        std::process::exit(if ok { 0 } else { 1 });
    }
    // `diskinfo list`, `diskinfo health`... run in the terminal without opening a window.
    // The quick fake-drive test runs a copy of this program as root (see rawcheck.rs).
    #[cfg(target_os = "linux")]
    if args.get(1).map(String::as_str) == Some("rawcheck") {
        std::process::exit(rawcheck::helper_main(&args));
    }
    if cli::wanted(&args) {
        std::process::exit(cli::run(&args));
    }
    let shot = arg_value(&args, "--screenshot").map(PathBuf::from);
    // --demo shows two sample disks with S.M.A.R.T. data (for checking the layout).
    let demo = args.iter().any(|a| a == "--demo");
    let tab = arg_value(&args, "--tab");
    let try_save = args.iter().any(|a| a == "--try-save");
    let try_bench = args.iter().any(|a| a == "--try-bench");
    let open_window = arg_value(&args, "--open");
    let disk: Option<usize> = arg_value(&args, "--disk").and_then(|v| v.parse().ok());
    // Keep the login entry pointing at this program (an AppImage may have moved).
    if shot.is_none() && !demo && Settings::load().autostart {
        autostart::set(true);
    }
    let minimized = args.iter().any(|a| a == "--minimized");
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
        .with_inner_size(arg_value(&args, "--window").and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?])
        }).unwrap_or([1020.0, 720.0]))
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
            if try_save {
                app.try_save = Some(0);
            }
            app.minimize_first = minimized && !app.quiet;
            // --open alerts opens the alert history at start (handy for checking the layout).
            if open_window.as_deref() == Some("alerts") {
                app.show_alerts = true;
            }
            if try_bench {
                app.try_bench = true;
                app.bench_open();
                app.bench.size_idx = 0;
                app.bench.secs = 1;
                app.bench.interval = 0;
                app.bench.auto_start = true;
            }
            if open_window.as_deref() == Some("fake") {
                app.fake_open();
                if demo {
                    app.fake.last = Some(demo::demo_fake());
                    app.fake.claimed = Some(128_000_000_000);
                    app.fake.free_at_start = 100_000_000_000;
                }
            }
            if open_window.as_deref() == Some("bench") {
                app.bench_open();
                if demo {
                    app.bench.last = Some(demo::demo_bench());
                }
            }
            // --disk N shows the Nth disk first (counting from 1).
            if let Some(n) = disk.and_then(|n| n.checked_sub(1)) {
                app.selected = n.min(app.disks.len().saturating_sub(1));
            }
            // --tab parts|details opens on that tab (handy for checking the layout).
            app.tab = match tab.as_deref() {
                Some("parts") => Tab::Partitions,
                Some("details") => Tab::Details,
                Some("history") => Tab::History,
                _ => Tab::Smart,
            };
            Ok(Box::new(app))
        }),
    )
}
