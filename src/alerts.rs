//! Desktop notifications when a disk's health gets worse or it passes its temperature alarm.
//! Uses what each system already has: notify-send (Linux), a toast via PowerShell (Windows),
//! `display notification` via osascript (macOS).

use crate::disks::Status;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Oldest events are dropped beyond this many.
const MAX_EVENTS: usize = 500;

/// One thing that was announced: a disk got worse or reached its temperature alarm.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    /// Seconds since 1970 (UTC).
    pub t: u64,
    /// "health" or "hot".
    pub kind: String,
    /// "Model (device)".
    pub disk: String,
    /// For "health": the new status key ("caution", "bad"); for "hot": the temperature in °C.
    pub value: String,
}

/// The history of alerts, kept on this computer next to the settings.
#[derive(Default, Serialize, Deserialize)]
pub struct Log {
    pub events: Vec<Event>,
}

impl Log {
    fn path() -> Option<std::path::PathBuf> {
        Some(crate::platform::config_dir()?.join("alerts.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(t) = serde_json::to_string(self) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(tmp, p);
            }
        }
    }

    pub fn push(&mut self, e: Event) {
        self.events.push(e);
        if self.events.len() > MAX_EVENTS {
            let extra = self.events.len() - MAX_EVENTS;
            self.events.drain(..extra);
        }
    }
}

/// Remembers what was last seen per disk, so each change is announced once.
#[derive(Default)]
pub struct Watcher {
    health: HashMap<String, Status>,
    hot: HashMap<String, bool>,
}

fn rank(s: Status) -> u8 {
    match s {
        Status::Unknown => 0,
        Status::Good => 1,
        Status::Caution => 2,
        Status::Bad => 3,
    }
}

impl Watcher {
    /// Returns true when the health got worse since the last check (not on the first one).
    pub fn health_worse(&mut self, key: &str, now: Status) -> bool {
        let before = self.health.insert(key.to_string(), now);
        matches!(before, Some(b) if rank(now) > rank(b) && rank(b) > 0)
    }

    /// Returns true when the temperature just reached the alarm. It re-arms 3 °C below it.
    pub fn too_hot(&mut self, key: &str, temp: Option<i32>, alarm: i32) -> bool {
        let Some(t) = temp else { return false };
        let was = self.hot.get(key).copied().unwrap_or(false);
        let is = if was { t > alarm - 3 } else { t >= alarm };
        self.hot.insert(key.to_string(), is);
        is && !was
    }
}

/// A short three-beep alarm as a WAV file (16-bit, mono, 22.05 kHz). It is made here, so no
/// sound file has to ship with the program.
pub fn alarm_wav() -> Vec<u8> {
    let rate = 22_050u32;
    let mut samples: Vec<i16> = Vec::new();
    for freq in [880.0_f32, 660.0, 880.0] {
        let n = (rate as f32 * 0.18) as usize;
        for k in 0..n {
            let t = k as f32 / rate as f32;
            // Quick fade in and out, so the beeps do not click.
            let fade = (k.min(n - k) as f32 / (rate as f32 * 0.01)).min(1.0);
            samples.push(((t * freq * std::f32::consts::TAU).sin() * 0.45 * fade * i16::MAX as f32) as i16);
        }
        samples.extend(std::iter::repeat(0).take((rate as f32 * 0.06) as usize));
    }
    let data_len = (samples.len() * 2) as u32;
    let mut w: Vec<u8> = Vec::with_capacity(44 + data_len as usize);
    w.extend(b"RIFF");
    w.extend((36 + data_len).to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes()); // size of the format block
    w.extend(1u16.to_le_bytes()); // plain PCM
    w.extend(1u16.to_le_bytes()); // mono
    w.extend(rate.to_le_bytes());
    w.extend((rate * 2).to_le_bytes()); // bytes per second
    w.extend(2u16.to_le_bytes()); // bytes per sample
    w.extend(16u16.to_le_bytes()); // bits per sample
    w.extend(b"data");
    w.extend(data_len.to_le_bytes());
    for s in samples {
        w.extend(s.to_le_bytes());
    }
    w
}

/// Plays the alarm; never blocks the window.
pub fn play_sound() {
    std::thread::spawn(|| {
        play_sound_blocking();
    });
}

/// Plays the alarm and waits for it to end. False when no way of playing it worked.
pub fn play_sound_blocking() -> bool {
    let path = std::env::temp_dir().join("diskinfo-alert.wav");
    let wav = alarm_wav();
    if std::fs::read(&path).ok().as_deref() != Some(&wav[..]) && std::fs::write(&path, &wav).is_err() {
        return false;
    }
    platform_play(&path)
}

/// Uses whatever sound player the system has (PulseAudio, PipeWire, ALSA or SoX).
#[cfg(target_os = "linux")]
fn platform_play(path: &std::path::Path) -> bool {
    let players: [(&str, &[&str]); 4] = [("paplay", &[]), ("pw-play", &[]), ("aplay", &["-q"]), ("play", &["-q"])];
    for (cmd, args) in players {
        let done = crate::platform::command(cmd)
            .args(args)
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if done.map(|s| s.success()).unwrap_or(false) {
            return true;
        }
    }
    false
}

#[cfg(windows)]
fn platform_play(path: &std::path::Path) -> bool {
    let file = path.display().to_string().replace('\'', "''");
    let ps = format!("(New-Object System.Media.SoundPlayer '{file}').PlaySync()");
    crate::platform::command("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn platform_play(path: &std::path::Path) -> bool {
    crate::platform::command("afplay").arg(path).status().map(|s| s.success()).unwrap_or(false)
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn platform_play(_path: &std::path::Path) -> bool {
    false
}

/// Shows a desktop notification; never blocks the window.
pub fn notify(title: &str, body: &str) {
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || show(&title, &body));
}

#[cfg(target_os = "linux")]
fn show(title: &str, body: &str) {
    let ok = crate::platform::command("notify-send")
        .args(["--app-name=DiskInfo", "--icon=diskinfo", title, body])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        // Without notify-send, talk to the notification service directly.
        let _ = crate::platform::command("gdbus")
            .args([
                "call", "--session", "--dest", "org.freedesktop.Notifications",
                "--object-path", "/org/freedesktop/Notifications",
                "--method", "org.freedesktop.Notifications.Notify",
                "DiskInfo", "0", "diskinfo", title, body, "[]", "{}", "10000",
            ])
            .status();
    }
}

#[cfg(windows)]
fn show(title: &str, body: &str) {
    let esc = |s: &str| s.replace('\'', "''").replace('&', "&amp;").replace('<', "&lt;");
    let ps = format!(
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
         $x = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
         $t = $x.GetElementsByTagName('text'); $t.Item(0).AppendChild($x.CreateTextNode('{}')) > $null; \
         $t.Item(1).AppendChild($x.CreateTextNode('{}')) > $null; \
         $id = '{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\\WindowsPowerShell\\v1.0\\powershell.exe'; \
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($id).Show([Windows.UI.Notifications.ToastNotification]::new($x))",
        esc(title),
        esc(body)
    );
    let _ = crate::platform::command("powershell").args(["-NoProfile", "-Command", &ps]).status();
}

#[cfg(target_os = "macos")]
fn show(title: &str, body: &str) {
    let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{}\" with title \"{}\"", q(body), q(title));
    let _ = crate::platform::command("osascript").args(["-e", &script]).status();
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn show(_title: &str, _body: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alarm_is_a_valid_wav() {
        let w = alarm_wav();
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(&w[36..40], b"data");
        let data_len = u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize;
        assert_eq!(w.len(), 44 + data_len, "header length matches the data");
        let seconds = data_len as f32 / 2.0 / 22_050.0;
        assert!((0.6..1.0).contains(&seconds), "about three quarters of a second: {seconds}");
    }

    #[test]
    fn announces_each_change_once() {
        let mut w = Watcher::default();
        assert!(!w.health_worse("d", Status::Good), "first sight is not a change");
        assert!(w.health_worse("d", Status::Caution));
        assert!(!w.health_worse("d", Status::Caution));
        assert!(!w.health_worse("d", Status::Good), "getting better is not announced");
        assert!(w.too_hot("d", Some(60), 60));
        assert!(!w.too_hot("d", Some(59), 60), "stays hot until 3 degrees below");
        assert!(!w.too_hot("d", Some(56), 60));
        assert!(w.too_hot("d", Some(60), 60));
    }
}
