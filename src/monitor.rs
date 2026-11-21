//! Watching the disks in the background: a thread reads them every few minutes, keeps the
//! history and announces problems, even while the window is minimised or hidden (a window
//! that is not drawn does not run its own code).
//!
//! The window and the thread share one `Core`. The window hands over what it shows
//! (`snapshot`, `direct_ok`, the settings) and picks up what the thread read (`pending`).

use crate::alerts::{self, Event, Log, Watcher};
use crate::disks::{self, Disk, ScanResult, Status};
use crate::history::{self, History};
use crate::i18n::t;
use crate::{health, Settings};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// What the thread read and the window has not applied yet.
pub enum Pending {
    /// A full reading (when S.M.A.R.T. can be read without asking for a password).
    Scan(ScanResult),
    /// Only temperatures, by device name (when S.M.A.R.T. needs administrator rights).
    Temps(Vec<(String, i32)>),
}

pub struct Core {
    pub history: History,
    pub watcher: Watcher,
    pub log: Log,
    /// A copy of the window's settings, kept up to date by the window.
    pub settings: Settings,
    /// A copy of the disks on screen, for the temperature-only readings.
    pub snapshot: Vec<Disk>,
    /// S.M.A.R.T. can be read as this user (no password needed).
    pub direct_ok: bool,
    pub pending: Option<Pending>,
}

pub type Shared = Arc<Mutex<Core>>;

pub fn new(settings: Settings, history: History, log: Log) -> Shared {
    Arc::new(Mutex::new(Core {
        history,
        watcher: Watcher::default(),
        log,
        settings,
        snapshot: Vec::new(),
        direct_ok: false,
        pending: None,
    }))
}

/// Locks the shared state (a panic in the other thread must not stop this one).
pub fn lock(shared: &Shared) -> MutexGuard<'_, Core> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

fn status_key(s: Status) -> &'static str {
    match s {
        Status::Good => "good",
        Status::Caution => "caution",
        Status::Bad => "bad",
        Status::Unknown => "unknown",
    }
}

impl Core {
    /// Records the disks in the history and announces what got worse or too hot.
    /// Safe to call twice with the same readings: each change is announced once.
    pub fn observe(&mut self, disks: &[Disk]) {
        let mut added = false;
        for d in disks {
            added |= self.history.record(d);
        }
        if added {
            self.history.save();
        }
        let lang = self.settings.language;
        let mut logged = false;
        for d in disks {
            let key = d.key();
            let label = format!("{} ({})", d.model, d.place());
            let th = self.settings.thresholds_for(d);
            if self.watcher.health_worse(&key, d.health) {
                self.log.push(Event {
                    t: history::now(),
                    kind: "health".into(),
                    disk: label.clone(),
                    value: status_key(d.health).into(),
                });
                logged = true;
                if self.settings.alerts {
                    let life = d.life.map(|l| format!(" ({l} %)")).unwrap_or_default();
                    alerts::notify(t(lang, "alert_health"), &format!("{label}: {}{life}", t(lang, status_key(d.health))));
                }
            }
            if self.watcher.too_hot(&key, d.temperature, th.alarm_temp) {
                let c = d.temperature.unwrap_or_default();
                self.log.push(Event { t: history::now(), kind: "hot".into(), disk: label.clone(), value: c.to_string() });
                logged = true;
                if self.settings.alerts {
                    alerts::notify(t(lang, "alert_hot"), &format!("{label}: {c} °C"));
                }
            }
        }
        if logged {
            self.log.save();
        }
    }
}

/// One reading of the thread: a full scan when S.M.A.R.T. can be read as this user,
/// otherwise only the temperatures of the disks the window shows.
pub fn step(shared: &Shared) {
    let (direct_ok, snapshot) = {
        let c = lock(shared);
        (c.direct_ok, c.snapshot.clone())
    };
    if direct_ok {
        let mut r = disks::scan(false);
        let mut c = lock(shared);
        for d in &mut r.disks {
            let th = c.settings.thresholds_for(d);
            health::evaluate(d, &th);
        }
        c.observe(&r.disks);
        c.pending = Some(Pending::Scan(r));
    } else if !snapshot.is_empty() {
        // Only temperatures can be read without administrator rights.
        let temps: Vec<(String, i32)> =
            snapshot.iter().filter_map(|d| disks::hwmon_temp(&d.dev).map(|t| (d.dev.clone(), t))).collect();
        let mut disks = snapshot;
        for (dev, temp) in &temps {
            if let Some(d) = disks.iter_mut().find(|d| &d.dev == dev) {
                d.temperature = Some(*temp);
            }
        }
        let mut c = lock(shared);
        c.observe(&disks);
        c.pending = Some(Pending::Temps(temps));
    }
}

/// Starts the watching thread. Call once, only for a real run (not for pictures or demos).
pub fn start(shared: Shared) {
    std::thread::spawn(move || {
        let mut last = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let every = lock(&shared).settings.auto_refresh_min;
            if every == 0 || last.elapsed() < Duration::from_secs(every as u64 * 60) {
                continue;
            }
            last = Instant::now();
            step(&shared);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps the tests from touching the real settings, history and alert files.
    fn private_config() {
        let dir = std::env::temp_dir().join(format!("diskinfo-test-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        std::env::set_var("APPDATA", &dir);
        std::env::set_var("HOME", &dir);
    }

    fn core() -> Shared {
        private_config();
        let mut settings = Settings::default();
        settings.alerts = false; // no desktop notification during a test
        new(settings, History::default(), Log::default())
    }

    fn disk(health: Status, temp: i32) -> Disk {
        Disk { dev: "sdx-not-real".into(), model: "Test".into(), serial: "T1".into(), health, temperature: Some(temp), ..Default::default() }
    }

    #[test]
    fn worse_health_and_heat_are_logged_once() {
        let shared = core();
        let mut c = lock(&shared);
        c.observe(&[disk(Status::Good, 40)]);
        assert!(c.log.events.is_empty(), "first sight is not a change");
        c.observe(&[disk(Status::Caution, 40)]);
        c.observe(&[disk(Status::Caution, 40)]);
        c.observe(&[disk(Status::Caution, 55)]);
        let kinds: Vec<&str> = c.log.events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["health", "hot"], "each change once: {kinds:?}");
        assert_eq!(c.log.events[0].value, "caution");
        assert_eq!(c.log.events[1].value, "55");
    }

    #[test]
    fn step_without_smart_reads_only_temperatures() {
        let shared = core();
        lock(&shared).snapshot = vec![disk(Status::Good, 40)];
        step(&shared);
        assert!(matches!(lock(&shared).pending, Some(Pending::Temps(_))));
    }
}
