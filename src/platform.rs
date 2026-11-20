//! Small differences between Linux, Windows and macOS.

use std::path::PathBuf;
use std::process::Command;

/// A command that never opens a console window on Windows.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// Where settings are kept.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("DiskInfo"))
    }
    #[cfg(target_os = "macos")]
    {
        home().map(|h| h.join("Library/Application Support/DiskInfo"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".config")))
            .map(|c| c.join("diskinfo"))
    }
}

/// Settings folder used before the app was renamed (Linux only).
pub fn legacy_config_dir() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".config")))
            .map(|c| c.join("info-discos"))
    } else {
        None
    }
}

/// Folder for files the app keeps for itself (a staged smartctl on Linux).
#[allow(dead_code)]
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".local/share")))
        .map(|d| d.join("diskinfo"))
}

/// True when this process can open disks directly (root, or an elevated Windows process).
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub fn is_admin() -> bool {
    #[cfg(windows)]
    {
        // `net session` only succeeds in an elevated process.
        command("net")
            .args(["session"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        command("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
            .unwrap_or(false)
    }
}

/// Windows: starts this program again with administrator rights (UAC prompt). True if started.
#[cfg(windows)]
pub fn relaunch_as_admin() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let exe = exe.to_string_lossy().replace('\'', "''");
    command("powershell")
        .args(["-NoProfile", "-Command", &format!("Start-Process -FilePath '{exe}' -Verb RunAs")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
