//! Command-line use: `diskinfo list`, `diskinfo health`, `diskinfo json`... without a window.
//! Same readings and health rules as the window; texts follow the saved language (or --lang).

use crate::disks::{self, Disk, Status};
use crate::i18n::{t, Lang};
use crate::{health, Settings};
use serde_json::{json, Value};

const COMMANDS: [&str; 8] = ["list", "health", "show", "partitions", "json", "selftest", "help", "version"];

/// True when the first argument is a command-line command (or --help / --version).
pub fn wanted(args: &[String]) -> bool {
    match args.get(1).map(String::as_str) {
        Some("-h" | "--help" | "-V" | "--version") => true,
        Some(a) => COMMANDS.contains(&a),
        None => false,
    }
}

/// Runs a command and returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    attach_console();
    let mut settings = Settings::load();
    let mut admin = false;
    let mut json_out = false;
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--admin" | "-a" => admin = true,
            "--json" | "-j" => json_out = true,
            "--lang" => {
                if let Some(l) = it.next().and_then(|v| lang_from(v)) {
                    settings.language = l;
                }
            }
            "-h" | "--help" => rest.insert(0, "help".into()),
            "-V" | "--version" => rest.insert(0, "version".into()),
            _ => rest.push(a.clone()),
        }
    }
    let lang = settings.language;
    let cmd = rest.first().map(String::as_str).unwrap_or("help");
    match cmd {
        "help" => {
            print!("{}", help_text(lang));
            0
        }
        "version" => {
            println!("{} {}", crate::APP_NAME, env!("CARGO_PKG_VERSION"));
            0
        }
        "list" | "health" | "show" | "partitions" | "json" => {
            let mut list = read(&settings, admin);
            if let Some(sel) = rest.get(1) {
                match pick(&list, sel) {
                    Some(i) => list = vec![list.swap_remove(i)],
                    None => {
                        eprintln!("{}: {sel}", t(lang, "cli_no_disk"));
                        return 2;
                    }
                }
            }
            if list.is_empty() {
                eprintln!("{}", t(lang, "no_disks"));
                return 3;
            }
            if json_out || cmd == "json" {
                println!("{}", serde_json::to_string_pretty(&to_json(&list, &settings)).unwrap_or_default());
            } else {
                match cmd {
                    "list" => print_list(&list, &settings),
                    "health" => print_health(&list, &settings),
                    "show" => list.iter().for_each(|d| print_show(d, &settings)),
                    _ => list.iter().for_each(|d| print_partitions(d, lang)),
                }
            }
            if !list.iter().any(|d| d.smart_read) && !admin {
                eprintln!("{}", t(lang, "cli_need_admin"));
            }
            exit_code(&list)
        }
        "selftest" => {
            let (Some(kind), Some(sel)) = (rest.get(1), rest.get(2)) else {
                eprintln!("{}", t(lang, "cli_selftest_usage"));
                return 2;
            };
            let extended = match kind.as_str() {
                "short" => false,
                "long" | "extended" => true,
                _ => {
                    eprintln!("{}", t(lang, "cli_selftest_usage"));
                    return 2;
                }
            };
            let list = read(&settings, admin);
            let Some(i) = pick(&list, sel) else {
                eprintln!("{}: {sel}", t(lang, "cli_no_disk"));
                return 2;
            };
            match disks::start_selftest(&list[i], extended) {
                Ok(()) => {
                    println!("{}", t(lang, "msg_test_started"));
                    0
                }
                Err(e) => {
                    eprintln!("{}", t(lang, e));
                    1
                }
            }
        }
        other => {
            eprintln!("{}: {other}\n", t(lang, "cli_unknown"));
            print!("{}", help_text(lang));
            2
        }
    }
}

/// On Windows the program is a window app; borrow the terminal it was started from.
fn attach_console() {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS);
    }
}

fn lang_from(v: &str) -> Option<Lang> {
    match v {
        "es" => Some(Lang::Es),
        "en" => Some(Lang::En),
        "pt" => Some(Lang::Pt),
        "fr" => Some(Lang::Fr),
        "de" => Some(Lang::De),
        "it" => Some(Lang::It),
        _ => None,
    }
}

fn read(settings: &Settings, admin: bool) -> Vec<Disk> {
    let mut r = disks::scan(false);
    if admin && !r.disks.iter().any(|d| d.smart_read) {
        let again = disks::scan(true);
        if !again.disks.is_empty() {
            r = again;
        }
    }
    let mut list = r.disks;
    for d in &mut list {
        let th = settings.thresholds_for(d);
        health::evaluate(d, &th);
    }
    list
}

/// A disk by number (1, 2...), device name (sda, nvme0n1) or path (/dev/sda, PhysicalDrive0).
fn pick(list: &[Disk], sel: &str) -> Option<usize> {
    if let Ok(n) = sel.parse::<usize>() {
        return n.checked_sub(1).filter(|i| *i < list.len());
    }
    let s = sel.trim_start_matches("/dev/");
    list.iter().position(|d| d.dev == s || d.place() == sel || d.place().trim_start_matches("/dev/") == s || d.serial == sel)
}

/// 0 all good, 1 some caution, 2 some bad, 3 nothing known.
fn exit_code(list: &[Disk]) -> i32 {
    if list.iter().any(|d| d.health == Status::Bad) {
        2
    } else if list.iter().any(|d| d.health == Status::Caution) {
        1
    } else if list.iter().all(|d| d.health == Status::Unknown) {
        3
    } else {
        0
    }
}

fn status_key(s: Status) -> &'static str {
    match s {
        Status::Good => "good",
        Status::Caution => "caution",
        Status::Bad => "bad",
        Status::Unknown => "unknown",
    }
}

fn size(b: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i < units.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", units[i])
    }
}

fn temp(d: &Disk, settings: &Settings) -> String {
    match d.temperature {
        Some(c) if settings.fahrenheit => format!("{} °F", c * 9 / 5 + 32),
        Some(c) => format!("{c} °C"),
        None => "--".into(),
    }
}

fn health_word(d: &Disk, lang: Lang) -> String {
    let w = t(lang, status_key(d.health));
    match d.life {
        Some(l) if d.health != Status::Unknown => format!("{w} ({l} %)"),
        _ => w.to_string(),
    }
}

fn print_list(list: &[Disk], s: &Settings) {
    let lang = s.language;
    println!(
        "{:<3} {:<16} {:<34} {:>10} {:<18} {:>7}",
        "#",
        t(lang, "d_path"),
        t(lang, "model"),
        t(lang, "col_size"),
        t(lang, "health_status"),
        t(lang, "temperature")
    );
    for (i, d) in list.iter().enumerate() {
        println!(
            "{:<3} {:<16} {:<34} {:>10} {:<18} {:>7}",
            i + 1,
            d.place(),
            cut(&d.model, 34),
            size(d.size_bytes),
            health_word(d, lang),
            temp(d, s)
        );
    }
}

fn print_health(list: &[Disk], s: &Settings) {
    for d in list {
        println!("{}\t{}\t{}\t{}", d.place(), t(s.language, status_key(d.health)), temp(d, s), d.model);
    }
}

fn print_show(d: &Disk, s: &Settings) {
    let lang = s.language;
    let line = "-".repeat(76);
    println!("{line}\n {} ({})\n{line}", d.model, d.place());
    let opt = |v: Option<u64>, unit: &str| v.map(|n| format!("{n}{unit}")).unwrap_or_else(|| "----".into());
    let rows = [
        ("firmware", d.firmware.clone()),
        ("serial", d.serial.clone()),
        ("capacity", size(d.size_bytes)),
        ("interface", d.interface.clone()),
        ("standard", d.standard.clone()),
        ("transfer_mode", d.transfer_mode.clone()),
        ("power_on_hours", opt(d.power_on_hours, "")),
        ("power_on_count", opt(d.power_on_count, "")),
        ("host_reads", opt(d.host_reads_gb, " GB")),
        ("host_writes", opt(d.host_writes_gb, " GB")),
        ("temperature", temp(d, s)),
        ("health_status", health_word(d, lang)),
        ("features", d.features.join(", ")),
        ("mounts", d.mounts.join(" ")),
    ];
    for (k, v) in rows {
        println!("{:>26} : {v}", t(lang, k));
    }
    for (k, v) in &d.details {
        println!("{:>26} : {}", t(lang, k), value_word(v, lang));
    }
    if !d.attrs.is_empty() {
        println!("\nID  {:>4} {:>4} {:>4} {:>14}  {}", t(lang, "col_current"), t(lang, "col_worst"), t(lang, "col_threshold"), t(lang, "col_raw"), t(lang, "col_name"));
        for a in &d.attrs {
            let n = |v: Option<u32>| v.map(|x| x.to_string()).unwrap_or_default();
            println!(
                "{:02X}  {:>4} {:>4} {:>4} {:>14X}  {}",
                a.id,
                n(a.current),
                n(a.worst),
                n(a.thresh),
                a.raw,
                crate::i18n::attr_name(lang, a.id, d.is_nvme, &a.name)
            );
        }
    }
    println!();
}

fn print_partitions(d: &Disk, lang: Lang) {
    println!("{} ({})", d.model, d.place());
    for p in &d.partitions {
        let usage = p.used_fraction().map(|f| format!("{:.0} %", f * 100.0)).unwrap_or_else(|| "—".into());
        println!(
            "  {:<14} {:>10} {:<20} {:>6}  {:<18} {}",
            p.name,
            size(p.size),
            cut(&p.fs, 20),
            usage,
            cut(&p.label, 18),
            p.mounts.join(" ")
        );
        for sv in &p.subs {
            println!("    · {:<30} {:<20} {}", cut(&sv.name, 30), t_key(lang, sv.kind), sv.mounts.join(" "));
        }
    }
    println!();
}

fn t_key(lang: Lang, key: &str) -> &'static str {
    crate::i18n::t_dyn(lang, key)
}

fn value_word(v: &str, lang: Lang) -> String {
    match v {
        "yes" => t(lang, "v_yes").into(),
        "no" => t(lang, "v_no").into(),
        "passed" => t(lang, "v_passed").into(),
        "failed" => t(lang, "v_failed").into(),
        other => other.into(),
    }
}

fn cut(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Everything known about the disks, for scripts and monitoring.
fn to_json(list: &[Disk], s: &Settings) -> Value {
    let disks: Vec<Value> = list
        .iter()
        .map(|d| {
            let th = s.thresholds_for(d);
            json!({
                "device": d.place(),
                "model": d.model,
                "serial": d.serial,
                "firmware": d.firmware,
                "size_bytes": d.size_bytes,
                "interface": d.interface,
                "ssd": d.is_ssd,
                "nvme": d.is_nvme,
                "health": status_key(d.health),
                "life_percent": d.life,
                "temperature_c": d.temperature,
                "temperature_alarm_c": th.alarm_temp,
                "temperature_status": status_key(health::temperature_status(d.temperature, &th)),
                "power_on_hours": d.power_on_hours,
                "power_on_count": d.power_on_count,
                "host_reads_gb": d.host_reads_gb,
                "host_writes_gb": d.host_writes_gb,
                "smart_read": d.smart_read,
                "features": d.features,
                "mounts": d.mounts,
                "attributes": d.attrs.iter().map(|a| json!({
                    "id": a.id,
                    "name": a.name,
                    "current": a.current,
                    "worst": a.worst,
                    "threshold": a.thresh,
                    "raw": a.raw,
                    "status": status_key(a.status),
                })).collect::<Vec<_>>(),
                "partitions": d.partitions.iter().map(|p| json!({
                    "name": p.name,
                    "label": p.label,
                    "fs": p.fs,
                    "size_bytes": p.size,
                    "used_bytes": p.used,
                    "free_bytes": p.free,
                    "mounts": p.mounts,
                    "type": p.kind,
                    "encryption": p.encryption,
                    "read_only": p.read_only,
                    "subvolumes": p.subs.iter().map(|sv| json!({
                        "name": sv.name, "kind": sv.kind.trim_start_matches("sub_"), "fs": sv.fs, "mounts": sv.mounts, "id": sv.id,
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "details": d.details.iter().map(|(k, v)| (k.trim_start_matches("d_").to_string(), Value::String(v.clone()))).collect::<serde_json::Map<_, _>>(),
                "selftest": {
                    "running_percent": d.selftest.running,
                    "status": d.selftest.status,
                    "log": d.selftest.log.iter().map(|(k, r, h)| json!({"type": k, "result": r, "hours": h})).collect::<Vec<_>>(),
                },
            })
        })
        .collect();
    json!({ "version": env!("CARGO_PKG_VERSION"), "disks": disks })
}

fn help_text(lang: Lang) -> String {
    format!(
        "{name} {ver}\n\n{usage}:\n  diskinfo                         {gui}\n  diskinfo list                    {list}\n  diskinfo health                  {health}\n  diskinfo show [DISK]             {show}\n  diskinfo partitions [DISK]       {parts}\n  diskinfo json [DISK]             {json}\n  diskinfo selftest short|long DISK  {test}\n  diskinfo help | version\n\n{opts}:\n  -a, --admin       {admin}\n  -j, --json        {jsonopt}\n  --lang es|en|pt|fr|de|it\n\nDISK: 1, 2… | sda | /dev/nvme0n1 | PhysicalDrive0 | {serial}\n{exit}: 0 = {good}, 1 = {caution}, 2 = {bad}, 3 = {unknown}\n",
        name = crate::APP_NAME,
        ver = env!("CARGO_PKG_VERSION"),
        usage = t(lang, "cli_usage"),
        gui = t(lang, "cli_gui"),
        list = t(lang, "cli_list"),
        health = t(lang, "cli_health"),
        show = t(lang, "cli_show"),
        parts = t(lang, "cli_parts"),
        json = t(lang, "cli_json"),
        test = t(lang, "cli_test"),
        opts = t(lang, "cli_options"),
        admin = t(lang, "cli_admin"),
        jsonopt = t(lang, "cli_json_opt"),
        serial = t(lang, "serial"),
        exit = t(lang, "cli_exit"),
        good = t(lang, "good"),
        caution = t(lang, "caution"),
        bad = t(lang, "bad"),
        unknown = t(lang, "unknown"),
    )
}
