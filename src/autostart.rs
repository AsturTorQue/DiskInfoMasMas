//! Starting DiskInfo++ when the person logs in, minimised, so the history keeps growing and
//! alerts keep coming without opening the window by hand.
//!
//! Linux: a file in ~/.config/autostart. Windows: a "Run" value in the registry (current user).
//! macOS: a LaunchAgent in ~/Library/LaunchAgents (starts at the next login).

use std::path::PathBuf;

#[cfg(not(target_os = "macos"))]
const NAME: &str = "DiskInfo++";

/// The file to start: the AppImage itself when running from one, otherwise this program.
fn program() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE").map(PathBuf::from).or_else(|| std::env::current_exe().ok())
}

/// Turns starting at login on or off. Returns false when it could not be done.
pub fn set(on: bool) -> bool {
    platform_set(on)
}

#[cfg(target_os = "linux")]
fn file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("autostart").join("diskinfo.desktop"))
}

#[cfg(target_os = "linux")]
fn platform_set(on: bool) -> bool {
    let Some(file) = file() else { return false };
    if !on {
        return match std::fs::remove_file(&file) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        };
    }
    let Some(program) = program() else { return false };
    let text = format!(
        "[Desktop Entry]\nType=Application\nName={NAME}\nExec={} --minimized\nIcon=diskinfo\nTerminal=false\nX-GNOME-Autostart-enabled=true\nX-DiskInfo=1\n",
        crate::integrate::exec_quote(&program.to_string_lossy())
    );
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&file, text).is_ok()
}

#[cfg(windows)]
fn platform_set(on: bool) -> bool {
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let mut cmd = crate::platform::command("reg");
    if on {
        let Some(program) = program() else { return false };
        let value = format!("\"{}\" --minimized", program.display());
        cmd.args(["add", KEY, "/v", NAME, "/t", "REG_SZ", "/d", &value, "/f"]);
    } else {
        cmd.args(["delete", KEY, "/v", NAME, "/f"]);
    }
    // Deleting something that is not there reports an error; that is fine.
    cmd.output().map(|o| o.status.success() || !on).unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn platform_set(on: bool) -> bool {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return false };
    let file = home.join("Library/LaunchAgents/io.github.asturtorque.diskinfomasmas.plist");
    if !on {
        return match std::fs::remove_file(&file) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        };
    }
    let Some(program) = program() else { return false };
    let esc = program.to_string_lossy().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>io.github.asturtorque.diskinfomasmas</string>\n  <key>ProgramArguments</key>\n  <array><string>{esc}</string><string>--minimized</string></array>\n  <key>RunAtLoad</key><true/>\n</dict>\n</plist>\n"
    );
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&file, text).is_ok()
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn platform_set(_on: bool) -> bool {
    false
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn creates_and_removes_the_login_entry() {
        let dir = std::env::temp_dir().join(format!("diskinfo-autostart-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        assert!(super::set(true));
        let text = std::fs::read_to_string(dir.join("autostart/diskinfo.desktop")).unwrap();
        assert!(text.contains("--minimized") && text.contains("Exec=\""), "{text}");
        assert!(super::set(false));
        assert!(!dir.join("autostart/diskinfo.desktop").exists());
        assert!(super::set(false), "removing twice is fine");
    }
}
