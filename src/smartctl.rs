//! Finding a smartctl to run: the system's, a copy this app installed, or the one bundled
//! with the app (inside the AppImage, next to DiskInfo.exe, or inside DiskInfo.app).
//!
//! Linux: the bundled one lives in the AppImage's private mount, which root cannot read, so for
//! administrator runs it is staged in the user's data folder and copied by root into a
//! root-owned temporary file whose SHA-256 must match the bundled original.

use std::path::{Path, PathBuf};

/// Root-owned copy installed together with the passwordless rule when the system has none.
#[cfg(target_os = "linux")]
pub const INSTALLED: &str = "/usr/local/libexec/diskinfo/smartctl";
#[cfg(target_os = "linux")]
pub const INSTALLED_DIR: &str = "/usr/local/libexec/diskinfo";
/// Where versions before the rename put it.
#[cfg(target_os = "linux")]
pub const LEGACY_INSTALLED_DIR: &str = "/usr/local/libexec/crystaldiskinfo-linux";

const EXE: &str = if cfg!(windows) { "smartctl.exe" } else { "smartctl" };

/// A program file only root can change (always true on Windows, where this doesn't apply).
pub fn root_owned(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(p)
            .map(|m| m.is_file() && m.uid() == 0 && m.mode() & 0o022 == 0 && m.mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

fn on_path() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).map(|d| d.join(EXE)).find(|p| p.is_file())
}

/// The system's smartctl, when installed (package manager, Homebrew, smartmontools installer).
pub fn system() -> Option<String> {
    let mut candidates: Vec<PathBuf> = on_path().into_iter().collect();
    if cfg!(target_os = "macos") {
        for p in ["/usr/local/sbin/smartctl", "/usr/local/bin/smartctl", "/opt/homebrew/sbin/smartctl", "/opt/homebrew/bin/smartctl"] {
            candidates.push(PathBuf::from(p));
        }
    }
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(pf) = std::env::var_os(var) {
                candidates.push(PathBuf::from(pf).join("smartmontools").join("bin").join(EXE));
            }
        }
    }
    candidates.into_iter().find_map(|p| {
        let real = std::fs::canonicalize(&p).ok()?;
        // On Linux only a root-owned copy may be handed to pkexec.
        let ok = if cfg!(target_os = "linux") { root_owned(&real) } else { real.is_file() };
        ok.then(|| display(&real))
    })
}

fn display(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    // canonicalize on Windows gives \\?\C:\..., which some programs dislike.
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

/// The copy installed by this app, when present (Linux).
#[cfg(target_os = "linux")]
pub fn installed() -> Option<String> {
    [INSTALLED, "/usr/local/libexec/crystaldiskinfo-linux/smartctl"]
        .into_iter()
        .find(|p| root_owned(Path::new(p)))
        .map(str::to_string)
}

/// The smartctl shipped with the app, or in a development build (target/smartctl).
pub fn bundled() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join(EXE), dir.join("..").join(EXE)].into_iter().find(|p| p.is_file())
}

/// A smartctl root can be trusted to run.
pub fn trusted() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        system().or_else(installed)
    }
    #[cfg(not(target_os = "linux"))]
    {
        system().or_else(|| bundled().map(|p| display(&p)))
    }
}

/// Any smartctl this user can start (it may still need administrator rights to open disks).
pub fn for_user() -> Option<String> {
    trusted().or_else(|| bundled().map(|p| display(&p)))
}

#[cfg(target_os = "linux")]
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Copies the bundled smartctl where root can read it. Returns (path, sha256 of the original).
#[cfg(target_os = "linux")]
pub fn stage_bundled() -> Option<(String, String)> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let src = bundled()?;
    let bytes = fs::read(&src).ok()?;
    let hash = sha256_hex(&bytes);
    let dir = crate::platform::data_dir()?;
    fs::create_dir_all(&dir).ok()?;
    let dst = dir.join("smartctl");
    if fs::read(&dst).ok().as_deref() != Some(bytes.as_slice()) {
        fs::write(&dst, &bytes).ok()?;
    }
    let _ = fs::set_permissions(&dst, fs::Permissions::from_mode(0o755));
    Some((dst.to_string_lossy().into_owned(), hash))
}

/// Shell lines (run as root) that make `$SMARTCTL` point to a smartctl safe to run.
/// `staged` is (path, sha256) from `stage_bundled`; with a trusted path that one is used.
#[cfg(target_os = "linux")]
pub fn root_prelude(trusted: Option<&str>, staged: Option<&(String, String)>) -> Option<String> {
    if let Some(t) = trusted {
        return Some(format!("SMARTCTL={}", shell_quote(t)));
    }
    let (path, hash) = staged?;
    Some(format!(
        "SMARTCTL=$(mktemp /run/diskinfo-smartctl.XXXXXX) || exit 9; trap 'rm -f \"$SMARTCTL\"' EXIT; \
         cat {src} > \"$SMARTCTL\" && chmod 700 \"$SMARTCTL\" && \
         echo \"{hash}  $SMARTCTL\" | sha256sum -c --quiet - || exit 9",
        src = shell_quote(path),
    ))
}

#[cfg(unix)]
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn quoting_and_prelude() {
        assert_eq!(shell_quote("/a b/it's"), "'/a b/it'\\''s'");
        let staged = ("/home/u/x".to_string(), "abc".to_string());
        let p = root_prelude(None, Some(&staged)).unwrap();
        assert!(p.contains("sha256sum -c"));
        assert!(p.contains("'/home/u/x'"));
        assert_eq!(root_prelude(Some("/usr/bin/smartctl"), None).unwrap(), "SMARTCTL='/usr/bin/smartctl'");
    }
}
