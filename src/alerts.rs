//! Desktop notifications when a disk's health gets worse or it passes its temperature alarm.
//! Uses what each system already has: notify-send (Linux), a toast via PowerShell (Windows),
//! `display notification` via osascript (macOS).

use crate::disks::Status;
use std::collections::HashMap;

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
