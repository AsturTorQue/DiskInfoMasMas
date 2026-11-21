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

/// The person's Documents folder (falls back to the home folder).
pub fn documents_dir() -> Option<PathBuf> {
    let home = home()?;
    #[cfg(target_os = "linux")]
    {
        // ~/.config/user-dirs.dirs says where "Documents" is in the person's language.
        let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
        if let Ok(text) = std::fs::read_to_string(base.join("user-dirs.dirs")) {
            for line in text.lines() {
                if let Some(v) = line.trim().strip_prefix("XDG_DOCUMENTS_DIR=") {
                    let path = PathBuf::from(v.trim_matches('"').replace("$HOME", &home.to_string_lossy()));
                    if path.is_dir() {
                        return Some(path);
                    }
                }
            }
        }
    }
    let docs = home.join("Documents");
    Some(if docs.is_dir() { docs } else { home })
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

/// Free space (bytes) available to the person on the disk that holds `dir`.
pub fn free_bytes(dir: &std::path::Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        // SAFETY: `path` is a valid C string and `st` is a plain struct that statvfs fills in.
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::statvfs(path.as_ptr(), &mut st) };
        if rc != 0 {
            return None;
        }
        Some(st.f_bavail as u64 * st.f_frsize as u64)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut free: u64 = 0;
        // SAFETY: `wide` is a NUL-terminated wide string and `free` a valid output location.
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut())
        };
        (ok != 0).then_some(free)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = dir;
        None
    }
}

/// Asks the person for a folder with the system's own dialog (zenity or kdialog on Linux, the
/// folder browser on Windows, Finder on macOS). Blocks until it is closed: run it in a thread.
/// `Ok(None)` means it was cancelled; `Err` that no dialog program was found.
pub fn pick_folder(title: &str) -> Result<Option<PathBuf>, String> {
    use std::process::Stdio;
    let run = |mut c: Command| -> Result<Option<PathBuf>, String> {
        let out = c.stdin(Stdio::null()).stderr(Stdio::null()).output().map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok((out.status.success() && !text.is_empty()).then(|| PathBuf::from(text)))
    };
    #[cfg(target_os = "linux")]
    {
        let start = home().map(|h| format!("{}/", h.display())).unwrap_or_default();
        let mut z = command("zenity");
        z.args(["--file-selection", "--directory", "--title", title, "--filename", &start]);
        match run(z) {
            Err(_) => {}
            ok => return ok,
        }
        let mut k = command("kdialog");
        k.args(["--title", title, "--getexistingdirectory", &start]);
        match run(k) {
            Err(_) => {}
            ok => return ok,
        }
        Err("zenity".into())
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = command("osascript");
        c.args(["-e", &format!("POSIX path of (choose folder with prompt \"{}\")", title.replace('"', ""))]);
        run(c)
    }
    #[cfg(windows)]
    {
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $f = New-Object System.Windows.Forms.FolderBrowserDialog; $f.Description = '{}'; if ($f.ShowDialog() -eq 'OK') {{ $f.SelectedPath }}",
            title.replace('\'', "")
        );
        let mut c = command("powershell");
        c.args(["-NoProfile", "-STA", "-Command", &script]);
        run(c)
    }
}
