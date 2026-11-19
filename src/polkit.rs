//! Optional polkit rule that lets the current user read S.M.A.R.T. without a password.
//!
//! The rule allows exactly `<smartctl> -j -a /dev/<name>` through pkexec, for this user,
//! only when they are sitting at the computer (local and active session). Nothing else.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub const RULE_PATH: &str = "/etc/polkit-1/rules.d/49-crystaldiskinfo-linux.rules";
/// Readable copy of the rule, so the app can tell it is installed.
pub const MARKER_PATH: &str = "/etc/crystaldiskinfo-linux/polkit-rule.js";

/// Absolute, resolved path of smartctl.
pub fn smartctl_path() -> Option<String> {
    let out = Command::new("sh").args(["-c", "command -v smartctl"]).output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if p.is_empty() {
        return None;
    }
    let real = std::fs::canonicalize(&p).unwrap_or_else(|_| PathBuf::from(&p));
    Some(real.to_string_lossy().into_owned())
}

fn user_name() -> Option<String> {
    let out = Command::new("id").arg("-un").output().ok()?;
    let u = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let ok = !u.is_empty() && u.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
    ok.then_some(u)
}

/// The command line pkexec runs for one disk (also what the rule allows).
pub fn read_args(smartctl: &str, dev: &str) -> [String; 4] {
    [smartctl.to_string(), "-j".into(), "-a".into(), format!("/dev/{dev}")]
}

fn rule_text(user: &str, smartctl: &str) -> String {
    let js = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"// Added by CrystalDiskInfo for Linux: lets {user} read disk health without a password.
// Only allows "smartctl -j -a /dev/<disk>" (read-only). Remove this file to undo.
polkit.addRule(function(action, subject) {{
    if (action.id != "org.freedesktop.policykit.exec") return polkit.Result.NOT_HANDLED;
    var prog = {prog};
    var prefix = prog + " -j -a /dev/";
    var cmd = action.lookup("command_line") || "";
    if (action.lookup("program") == prog &&
        subject.user == {user_js} && subject.local && subject.active &&
        cmd.indexOf(prefix) == 0 &&
        /^[A-Za-z0-9]+$/.test(cmd.substring(prefix.length))) {{
        return polkit.Result.YES;
    }}
    return polkit.Result.NOT_HANDLED;
}});
"#,
        prog = js(smartctl),
        user_js = js(user),
    )
}

/// True when the rule is installed for this user. The rules folder is only readable by
/// root, so installing also leaves a world-readable copy of the rule as a marker.
pub fn rule_active(_sample_dev: &str) -> bool {
    let (Some(user), Ok(text)) = (user_name(), std::fs::read_to_string(MARKER_PATH)) else {
        return false;
    };
    let js_user = serde_json::to_string(&user).unwrap_or_default();
    text.contains(&format!("subject.user == {js_user}"))
}

/// Installs the rule (one password prompt).
pub fn install() -> Result<(), &'static str> {
    let smartctl = smartctl_path().ok_or("msg_no_smartctl")?;
    let user = user_name().ok_or("msg_rule_failed")?;
    let text = rule_text(&user, &smartctl);
    let script = format!(
        "umask 022; mkdir -p /etc/polkit-1/rules.d /etc/crystaldiskinfo-linux && cat > {RULE_PATH} && cp {RULE_PATH} {MARKER_PATH}"
    );
    let mut child = Command::new("pkexec")
        .args(["/bin/sh", "-c", &script])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "msg_no_pkexec")?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let status = child.wait().map_err(|_| "msg_rule_failed")?;
    if status.success() {
        Ok(())
    } else {
        Err("msg_rule_failed")
    }
}

/// Removes the rule (one password prompt).
pub fn uninstall() -> Result<(), &'static str> {
    let status = Command::new("pkexec")
        .args(["/bin/rm", "-f", RULE_PATH, MARKER_PATH])
        .stdin(Stdio::null())
        .status()
        .map_err(|_| "msg_no_pkexec")?;
    if status.success() {
        Ok(())
    } else {
        Err("msg_rule_failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_mentions_only_smartctl_read() {
        let r = rule_text("efren", "/usr/bin/smartctl");
        assert!(r.contains(r#"var prog = "/usr/bin/smartctl";"#));
        assert!(r.contains(r#"subject.user == "efren""#));
        assert!(r.contains(r#" -j -a /dev/"#));
    }
}

#[cfg(test)]
mod print_rule {
    #[test]
    #[ignore]
    fn print() {
        println!("{}", super::rule_text("efren", "/usr/bin/smartctl"));
    }
}
