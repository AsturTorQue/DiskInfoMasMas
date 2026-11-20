//! Optional polkit rule that lets the current user read S.M.A.R.T. without a password.
//!
//! The rule allows exactly `<smartctl> -j -a /dev/<name>` through pkexec, for this user,
//! only when they are sitting at the computer (local and active session). Nothing else.

use crate::smartctl::{self, shell_quote, INSTALLED, INSTALLED_DIR, LEGACY_INSTALLED_DIR};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub const RULE_PATH: &str = "/etc/polkit-1/rules.d/49-diskinfo.rules";
/// Readable copy of the rule, so the app can tell it is installed.
pub const MARKER_PATH: &str = "/etc/diskinfo/polkit-rule.js";
/// Where versions from before the rename put them; still honoured and removed on uninstall.
const LEGACY_RULE_PATH: &str = "/etc/polkit-1/rules.d/49-crystaldiskinfo-linux.rules";
const LEGACY_MARKER_PATH: &str = "/etc/crystaldiskinfo-linux/polkit-rule.js";

fn user_name() -> Option<String> {
    let out = Command::new("id").arg("-un").output().ok()?;
    let u = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let ok = !u.is_empty() && u.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
    ok.then_some(u)
}

/// The command line pkexec runs for one disk (also what the rule allows). `-x` reads more
/// (device statistics, NVMe power states...); rules made by older versions only allow `-a`.
pub fn read_args(smartctl: &str, dev: &str) -> [String; 4] {
    let mode = if rule_allows_extended() { "-x" } else { "-a" };
    [smartctl.to_string(), "-j".into(), mode.into(), format!("/dev/{dev}")]
}

fn marker_text() -> Option<String> {
    std::fs::read_to_string(MARKER_PATH)
        .or_else(|_| std::fs::read_to_string(LEGACY_MARKER_PATH))
        .ok()
}

/// Whether the installed rule also allows `smartctl -j -x` (rules from version 0.3 on).
pub fn rule_allows_extended() -> bool {
    marker_text().is_some_and(|t| t.contains("\" -j -x /dev/\""))
}

fn rule_text(user: &str, smartctl: &str) -> String {
    let js = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"// Added by DiskInfo: lets {user} read disk health without a password.
// Only allows "smartctl -j -a /dev/<disk>" and "smartctl -j -x /dev/<disk>" (both only read).
// Remove this file to undo.
polkit.addRule(function(action, subject) {{
    if (action.id != "org.freedesktop.policykit.exec") return polkit.Result.NOT_HANDLED;
    var prog = {prog};
    var cmd = action.lookup("command_line") || "";
    var modes = [" -j -a /dev/", " -j -x /dev/"];
    if (action.lookup("program") != prog || subject.user != {user_js} || !subject.local || !subject.active) {{
        return polkit.Result.NOT_HANDLED;
    }}
    for (var i = 0; i < modes.length; i++) {{
        var prefix = prog + modes[i];
        if (cmd.indexOf(prefix) == 0 && /^[A-Za-z0-9]+$/.test(cmd.substring(prefix.length))) {{
            return polkit.Result.YES;
        }}
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
    rule_program().is_some()
}

/// The smartctl the installed rule allows, when the rule is for this user and that file is
/// still a root-owned program.
pub fn rule_program() -> Option<String> {
    let user = user_name()?;
    let text = marker_text()?;
    let js_user = serde_json::to_string(&user).ok()?;
    // Older rules say `subject.user == "name"`, newer ones `subject.user != "name"`.
    let mine = text.contains(&format!("subject.user == {js_user}")) || text.contains(&format!("subject.user != {js_user}"));
    if !mine {
        return None;
    }
    let line = text.lines().find_map(|l| l.trim().strip_prefix("var prog = "))?;
    let prog: String = serde_json::from_str(line.trim_end_matches(';')).ok()?;
    smartctl::root_owned(Path::new(&prog)).then_some(prog)
}

/// Installs the rule (one password prompt). Without a system smartctl, the bundled one is
/// first copied to a root-owned place (after checking its SHA-256), and the rule allows that.
pub fn install() -> Result<(), &'static str> {
    let user = user_name().ok_or("msg_rule_failed")?;
    let (prog, copy) = match smartctl::system() {
        Some(p) => (p, String::new()),
        None => {
            let (src, hash) = smartctl::stage_bundled().ok_or("msg_no_smartctl")?;
            let copy = format!(
                "mkdir -p {dir} && cat {src} > {tmp} && echo \"{hash}  {tmp}\" | sha256sum -c --quiet - && \
                 chown root:root {tmp} && chmod 755 {tmp} && mv -f {tmp} {dst} && ",
                dir = INSTALLED_DIR,
                src = shell_quote(&src),
                tmp = format!("{INSTALLED}.new"),
                dst = INSTALLED,
            );
            (INSTALLED.to_string(), copy)
        }
    };
    let text = rule_text(&user, &prog);
    let script = format!(
        "umask 022; {copy}mkdir -p /etc/polkit-1/rules.d /etc/diskinfo && rm -f {LEGACY_RULE_PATH} {LEGACY_MARKER_PATH} && cat > {RULE_PATH} && cp {RULE_PATH} {MARKER_PATH}"
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
        .args(["/bin/rm", "-rf", RULE_PATH, MARKER_PATH, INSTALLED_DIR, LEGACY_RULE_PATH, LEGACY_MARKER_PATH, LEGACY_INSTALLED_DIR])
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
        assert!(r.contains(r#"subject.user != "efren""#));
        assert!(r.contains(r#"" -j -a /dev/""#));
        assert!(r.contains(r#"" -j -x /dev/""#));
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
