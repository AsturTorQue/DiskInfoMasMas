//! Finding a smartctl to run: the system's, a copy this app installed, or the one bundled
//! inside the AppImage.
//!
//! The bundled one lives in the AppImage's private mount, which root cannot read, so for
//! administrator runs it is staged in the user's data folder and copied by root into a
//! root-owned temporary file whose SHA-256 must match the bundled original.

use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Root-owned copy installed together with the passwordless rule when the system has none.
pub const INSTALLED: &str = "/usr/local/libexec/crystaldiskinfo-linux/smartctl";
pub const INSTALLED_DIR: &str = "/usr/local/libexec/crystaldiskinfo-linux";

/// A program file owned by root that only root can change.
pub fn root_owned(p: &Path) -> bool {
    fs::metadata(p)
        .map(|m| m.is_file() && m.uid() == 0 && m.mode() & 0o022 == 0 && m.mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// The distribution's smartctl, when installed.
pub fn system() -> Option<String> {
    let out = Command::new("sh").args(["-c", "command -v smartctl"]).output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if p.is_empty() {
        return None;
    }
    let real = fs::canonicalize(&p).ok()?;
    root_owned(&real).then(|| real.to_string_lossy().into_owned())
}

/// The copy installed by this app, when present.
pub fn installed() -> Option<String> {
    root_owned(Path::new(INSTALLED)).then(|| INSTALLED.to_string())
}

/// The smartctl shipped next to this program (inside the AppImage), or in a development build.
pub fn bundled() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("smartctl"), dir.join("../smartctl")]
        .into_iter()
        .find(|p| p.is_file())
}

/// A smartctl root can be trusted to run: system or installed copy.
pub fn trusted() -> Option<String> {
    system().or_else(installed)
}

/// Any smartctl this user can start (it still needs root to open the disks).
pub fn for_user() -> Option<String> {
    trusted().or_else(|| bundled().map(|p| p.to_string_lossy().into_owned()))
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Copies the bundled smartctl where root can read it. Returns (path, sha256 of the original).
pub fn stage_bundled() -> Option<(String, String)> {
    let src = bundled()?;
    let bytes = fs::read(&src).ok()?;
    let hash = sha256_hex(&bytes);
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    let dir = base.join("info-discos");
    fs::create_dir_all(&dir).ok()?;
    let dst = dir.join("smartctl");
    if fs::read(&dst).ok().as_deref() != Some(bytes.as_slice()) {
        fs::write(&dst, &bytes).ok()?;
    }
    let _ = fs::set_permissions(&dst, fs::Permissions::from_mode(0o755));
    Some((dst.to_string_lossy().into_owned(), hash))
}

/// Shell lines (run as root) that make `$SMARTCTL` point to a smartctl safe to run.
/// `staged` is (path, sha256) from `stage_bundled`; with None the trusted path is used.
pub fn root_prelude(trusted: Option<&str>, staged: Option<&(String, String)>) -> Option<String> {
    if let Some(t) = trusted {
        return Some(format!("SMARTCTL={}", shell_quote(t)));
    }
    let (path, hash) = staged?;
    Some(format!(
        "SMARTCTL=$(mktemp /run/cdi-smartctl.XXXXXX) || exit 9; trap 'rm -f \"$SMARTCTL\"' EXIT; \
         cat {src} > \"$SMARTCTL\" && chmod 700 \"$SMARTCTL\" && \
         echo \"{hash}  $SMARTCTL\" | sha256sum -c --quiet - || exit 9",
        src = shell_quote(path),
    ))
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
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
