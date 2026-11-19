//! Adds the program to the user's application menu (no administrator rights needed).
//!
//! On Wayland the window icon comes from the menu entry whose name matches the window's
//! app id ("info-discos"), so without this entry the taskbar shows a generic icon.

use std::fs;
use std::path::PathBuf;

const APP_ID: &str = "info-discos";
const ICON_PNG: &[u8] = include_bytes!("../packaging/info-discos.png");

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

fn desktop_path() -> Option<PathBuf> {
    Some(data_home()?.join("applications").join(format!("{APP_ID}.desktop")))
}

fn icon_path() -> Option<PathBuf> {
    Some(data_home()?.join("icons/hicolor/256x256/apps").join(format!("{APP_ID}.png")))
}

/// The file to start: the AppImage itself when running from one, otherwise this program.
fn program_path() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
}

/// Quotes a path for the Exec line of a .desktop file.
fn exec_quote(p: &str) -> String {
    let mut s = String::from("\"");
    for c in p.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    s
}

fn entry_text(program: &str, icon: &str, comment: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={}\nComment={comment}\nExec={}\nIcon={icon}\nCategories=System;Monitor;\nTerminal=false\nStartupWMClass={APP_ID}\nX-CrystalDiskInfo-Linux=1\n",
        crate::APP_NAME,
        exec_quote(program)
    )
}

/// Writes (or refreshes) the menu entry and the icon. Cheap; rewrites only on change.
pub fn install(comment: &str) -> bool {
    let (Some(desktop), Some(icon), Some(program)) = (desktop_path(), icon_path(), program_path()) else {
        return false;
    };
    if let Some(dir) = icon.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if fs::read(&icon).ok().as_deref() != Some(ICON_PNG) && fs::write(&icon, ICON_PNG).is_err() {
        return false;
    }
    let text = entry_text(&program.to_string_lossy(), &icon.to_string_lossy(), comment);
    if fs::read_to_string(&desktop).ok().as_deref() == Some(text.as_str()) {
        return true;
    }
    if let Some(dir) = desktop.parent() {
        let _ = fs::create_dir_all(dir);
    }
    fs::write(&desktop, text).is_ok()
}

/// Removes the menu entry and icon written by `install`.
pub fn uninstall() {
    if let Some(p) = desktop_path() {
        // Only remove an entry this program wrote.
        if fs::read_to_string(&p).map(|t| t.contains("X-CrystalDiskInfo-Linux=1")).unwrap_or(false) {
            let _ = fs::remove_file(p);
        }
    }
    if let Some(p) = icon_path() {
        if fs::read(&p).ok().as_deref() == Some(ICON_PNG) {
            let _ = fs::remove_file(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_line_quotes_spaces() {
        let t = entry_text("/home/a b/CrystalDiskInfo.AppImage", "/x/i.png", "c");
        assert!(t.contains("Exec=\"/home/a b/CrystalDiskInfo.AppImage\"\n"));
        assert!(t.contains("StartupWMClass=info-discos"));
    }
}
