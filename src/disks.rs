//! Finding disks and reading their S.M.A.R.T. data (Linux, Windows and macOS).

use serde_json::Value;
use std::fs;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Status {
    #[default]
    Unknown,
    Good,
    Caution,
    Bad,
}

#[derive(Clone, Debug, Default)]
pub struct Attr {
    pub id: u16,
    pub name: String,
    pub current: Option<u32>,
    pub worst: Option<u32>,
    pub thresh: Option<u32>,
    pub raw: u64,
    pub status: Status,
}

#[derive(Clone, Debug, Default)]
pub struct Disk {
    pub dev: String,
    pub model: String,
    pub serial: String,
    pub firmware: String,
    pub size_bytes: u64,
    pub is_ssd: bool,
    pub is_nvme: bool,
    pub interface: String,
    pub standard: String,
    pub transfer_mode: String,
    pub rotation_rpm: Option<u32>,
    pub power_on_hours: Option<u64>,
    pub power_on_count: Option<u64>,
    pub host_reads_gb: Option<u64>,
    pub host_writes_gb: Option<u64>,
    pub temperature: Option<i32>,
    pub features: Vec<String>,
    pub attrs: Vec<Attr>,
    pub life: Option<i32>,
    pub life_attr: Option<u16>,
    pub nvme_critical: Option<u64>,
    pub nvme_spare: Option<i64>,
    pub nvme_spare_thresh: Option<i64>,
    pub smart_read: bool,
    pub mounts: Vec<String>,
    pub health: Status,
    /// Device smartctl opens on Windows/macOS ("/dev/sda", "/dev/disk0"); empty on Linux.
    #[allow(dead_code)]
    pub smart_dev: String,
    /// smartctl's -d type for it, when known.
    #[allow(dead_code)]
    pub smart_type: Option<String>,
    /// How the disk is named to the person ("PhysicalDrive0", "/dev/disk0"); empty on Linux.
    pub path_label: String,
}

impl Disk {
    /// The disk's name in the system, for display.
    pub fn place(&self) -> String {
        if self.path_label.is_empty() {
            format!("/dev/{}", self.dev)
        } else {
            self.path_label.clone()
        }
    }

    /// Key used to remember per-disk settings.
    pub fn key(&self) -> String {
        format!("{}{}", self.model, self.serial)
    }
}

pub struct ScanResult {
    pub disks: Vec<Disk>,
    /// S.M.A.R.T. could not be read without administrator rights.
    pub needs_admin: bool,
    /// Text key from i18n.
    pub message: Option<&'static str>,
    /// S.M.A.R.T. was read through the passwordless polkit rule.
    pub passwordless: bool,
}

#[cfg(target_os = "linux")]
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn read_trim(p: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(target_os = "linux")]
/// Lists physical disks with what Linux tells any user, without S.M.A.R.T.
pub fn list_disks() -> Vec<Disk> {
    let Some(txt) = run(
        "lsblk",
        &["-J", "-b", "-o", "NAME,MODEL,SERIAL,SIZE,ROTA,TRAN,TYPE,REV,MOUNTPOINTS"],
    ) else {
        return Vec::new();
    };
    let Ok(j) = serde_json::from_str::<Value>(&txt) else {
        return Vec::new();
    };
    let mut disks = Vec::new();
    for b in j["blockdevices"].as_array().into_iter().flatten() {
        let name = b["name"].as_str().unwrap_or_default().to_string();
        if b["type"].as_str() != Some("disk")
            || ["zram", "loop", "ram", "sr", "fd"].iter().any(|p| name.starts_with(p))
        {
            continue;
        }
        let tran = b["tran"].as_str().unwrap_or("").to_string();
        let mut d = Disk {
            dev: name.clone(),
            model: b["model"].as_str().unwrap_or("Disco").trim().to_string(),
            serial: b["serial"].as_str().unwrap_or("").trim().to_string(),
            firmware: b["rev"].as_str().unwrap_or("").trim().to_string(),
            size_bytes: b["size"].as_u64().unwrap_or(0),
            is_ssd: !as_bool(&b["rota"]),
            is_nvme: tran == "nvme" || name.starts_with("nvme"),
            ..Default::default()
        };
        d.interface = match tran.as_str() {
            "nvme" => "NVM Express".into(),
            "sata" | "ata" => "Serial ATA".into(),
            "usb" => "USB".into(),
            "" => "—".into(),
            other => other.to_uppercase(),
        };
        collect_mounts(b, &mut d.mounts);
        if d.is_nvme {
            d.transfer_mode = pcie_mode(&name);
            if let Some(fw) = read_trim(format!("/sys/block/{name}/device/firmware_rev")) {
                d.firmware = fw;
            }
        }
        d.temperature = hwmon_temp(&name);
        disks.push(d);
    }
    disks
}

#[cfg(target_os = "linux")]
fn as_bool(v: &Value) -> bool {
    v.as_bool().unwrap_or_else(|| v.as_str() == Some("1") || v.as_u64() == Some(1))
}

#[cfg(target_os = "linux")]
fn collect_mounts(node: &Value, out: &mut Vec<String>) {
    for m in node["mountpoints"].as_array().into_iter().flatten() {
        if let Some(s) = m.as_str() {
            if !s.starts_with('[') && !out.iter().any(|o| o == s) {
                out.push(s.to_string());
            }
        }
    }
    for c in node["children"].as_array().into_iter().flatten() {
        collect_mounts(c, out);
    }
}

#[cfg(target_os = "linux")]
/// "PCIe 3.0 x4 | PCIe 4.0 x4" (current | supported) from the PCI link.
fn pcie_mode(dev: &str) -> String {
    let base = format!("/sys/block/{dev}/device/device");
    let gen = |speed: Option<String>| -> String {
        let gt: f32 = speed
            .and_then(|s| s.split_whitespace().next().and_then(|n| n.parse().ok()))
            .unwrap_or(0.0);
        match gt as u32 {
            2 => "1.0",
            5 => "2.0",
            8 => "3.0",
            16 => "4.0",
            32 => "5.0",
            64 => "6.0",
            _ => "?",
        }
        .to_string()
    };
    let cur = read_trim(format!("{base}/current_link_speed"));
    if cur.is_none() {
        return "—".into();
    }
    let cw = read_trim(format!("{base}/current_link_width")).unwrap_or_default();
    let max = read_trim(format!("{base}/max_link_speed"));
    let mw = read_trim(format!("{base}/max_link_width")).unwrap_or_default();
    format!("PCIe {} x{} | PCIe {} x{}", gen(cur), cw, gen(max), mw)
}

/// Temperature any user can read from the kernel (nvme and drivetemp sensors).
pub fn hwmon_temp(dev: &str) -> Option<i32> {
    let roots = [
        format!("/sys/block/{dev}/device"),
        format!("/sys/block/{dev}/device/hwmon"),
        format!("/sys/block/{dev}/device/device"),
    ];
    for root in roots {
        let Ok(entries) = fs::read_dir(&root) else { continue };
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with("hwmon") {
                if let Some(t) = read_trim(e.path().join("temp1_input")) {
                    if let Ok(milli) = t.parse::<i64>() {
                        return Some((milli / 1000) as i32);
                    }
                }
            }
        }
    }
    None
}

enum SmartRead {
    Ok(Value),
    Denied,
    Missing,
    Failed,
}

/// Runs `smartctl -j -a` as this user.
fn smartctl_read(path: &str, dtype: Option<&str>) -> SmartRead {
    let Some(smartctl) = crate::smartctl::for_user() else {
        return SmartRead::Missing;
    };
    let mut cmd = crate::platform::command(smartctl);
    cmd.args(["-j", "-a"]);
    if let Some(t) = dtype {
        cmd.args(["-d", t]);
    }
    let out = match cmd.arg(path).output() {
        Ok(o) => o,
        Err(_) => return SmartRead::Missing,
    };
    let Ok(j) = serde_json::from_slice::<Value>(&out.stdout) else {
        return SmartRead::Failed;
    };
    let denied = j["smartctl"]["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|m| {
            let s = m["string"].as_str().unwrap_or("");
            s.contains("Permission denied") || s.contains("Access is denied") || s.contains("Operation not permitted")
        });
    if denied {
        SmartRead::Denied
    } else if j.get("device").is_some() {
        SmartRead::Ok(j)
    } else {
        SmartRead::Failed
    }
}

/// Full scan. With `admin`, one administrator prompt reads every disk (Linux and macOS).
pub fn scan(admin: bool) -> ScanResult {
    #[cfg(target_os = "linux")]
    {
        scan_linux(admin)
    }
    #[cfg(not(target_os = "linux"))]
    {
        scan_other(admin)
    }
}

#[cfg(target_os = "linux")]
fn scan_linux(admin: bool) -> ScanResult {
    let mut disks = list_disks();
    let mut needs_admin = false;
    let mut message = None;
    let mut passwordless = false;
    if admin {
        let names: Vec<String> = disks.iter().map(|d| d.dev.clone()).collect();
        match smartctl_admin(&names) {
            Ok(list) => {
                for (dev, j) in list {
                    if let Some(d) = disks.iter_mut().find(|d| d.dev == dev) {
                        parse_smart(d, &j);
                    }
                }
            }
            Err(e) => {
                needs_admin = true;
                message = Some(e);
            }
        }
    } else {
        for d in &mut disks {
            match smartctl_read(&format!("/dev/{}", d.dev), None) {
                SmartRead::Ok(j) => parse_smart(d, &j),
                SmartRead::Denied => needs_admin = true,
                SmartRead::Missing => message = Some("msg_no_smartctl"),
                SmartRead::Failed => {}
            }
        }
        // With the optional polkit rule in place, read each disk without a password.
        if needs_admin && !disks.is_empty() && crate::polkit::rule_active(&disks[0].dev) {
            passwordless = true;
            needs_admin = false;
            for d in &mut disks {
                if let Some(j) = smartctl_passwordless(&d.dev) {
                    parse_smart(d, &j);
                }
            }
        }
    }
    ScanResult { disks, needs_admin, message, passwordless }
}

#[cfg(target_os = "linux")]
fn smartctl_passwordless(dev: &str) -> Option<Value> {
    if !dev.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let smartctl = crate::polkit::rule_program()?;
    let out = Command::new("pkexec")
        .args(crate::polkit::read_args(&smartctl, dev))
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let j: Value = serde_json::from_slice(&out.stdout).ok()?;
    j.get("device").is_some().then_some(j)
}

#[cfg(target_os = "linux")]
fn smartctl_admin(devs: &[String]) -> Result<Vec<(String, Value)>, &'static str> {
    let safe: Vec<&String> = devs
        .iter()
        .filter(|d| d.chars().all(|c| c.is_ascii_alphanumeric()))
        .collect();
    let trusted = crate::smartctl::trusted();
    let staged = if trusted.is_none() { crate::smartctl::stage_bundled() } else { None };
    let prelude = crate::smartctl::root_prelude(trusted.as_deref(), staged.as_ref()).ok_or("msg_no_smartctl")?;
    let script = format!(r#"{prelude}; for d in "$@"; do echo "@@DEV $d"; "$SMARTCTL" -j -a "/dev/$d"; done"#);
    let out = Command::new("pkexec")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg("sh")
        .args(safe.iter().map(|s| s.as_str()))
        .output()
        .map_err(|_| "msg_no_pkexec")?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !text.contains("@@DEV") {
        return Err("msg_denied");
    }
    Ok(split_dev_chunks(&text))
}

/// Splits "@@DEV <name>\n<json>" blocks.
#[allow(dead_code)]
fn split_dev_chunks(text: &str) -> Vec<(String, Value)> {
    let mut res = Vec::new();
    for chunk in text.split("@@DEV ").skip(1) {
        let (dev, body) = chunk.split_once('\n').unwrap_or((chunk, ""));
        if let Ok(j) = serde_json::from_str::<Value>(body) {
            if j.get("device").is_some() {
                res.push((dev.trim().to_string(), j));
            }
        }
    }
    res
}

/// Windows and macOS: disks come from smartctl itself (plus Windows' own disk list).
#[cfg(not(target_os = "linux"))]
fn scan_other(admin: bool) -> ScanResult {
    let mut result = ScanResult { disks: Vec::new(), needs_admin: false, message: None, passwordless: false };
    let Some(smartctl) = crate::smartctl::for_user() else {
        result.message = Some("msg_no_smartctl");
        return result;
    };
    #[cfg(target_os = "macos")]
    if admin {
        match mac_admin_read(&smartctl) {
            Ok(list) => {
                for (name, j) in list {
                    let mut d = disk_for(&name, None);
                    parse_smart(&mut d, &j);
                    result.disks.push(d);
                }
                return result;
            }
            Err(e) => result.message = Some(e),
        }
    }
    let _ = admin;
    for (name, dtype) in scan_devices(&smartctl) {
        let mut d = disk_for(&name, dtype.clone());
        match smartctl_read(&name, dtype.as_deref()) {
            SmartRead::Ok(j) => parse_smart(&mut d, &j),
            SmartRead::Denied => result.needs_admin = true,
            _ => {}
        }
        result.disks.push(d);
    }
    #[cfg(windows)]
    windows_merge(&mut result);
    if result.disks.iter().all(|d| !d.smart_read) && !crate::platform::is_admin() {
        result.needs_admin = true;
    }
    for d in &mut result.disks {
        if d.model.is_empty() {
            d.model = d.path_label.clone();
        }
    }
    result
}

#[cfg(not(target_os = "linux"))]
fn disk_for(name: &str, dtype: Option<String>) -> Disk {
    let short = name.trim_start_matches("/dev/").to_string();
    Disk {
        dev: short,
        smart_dev: name.to_string(),
        smart_type: dtype,
        path_label: name.to_string(),
        ..Default::default()
    }
}

/// `smartctl --scan-open -j`: device names and types.
#[cfg(not(target_os = "linux"))]
fn scan_devices(smartctl: &str) -> Vec<(String, Option<String>)> {
    let Ok(out) = crate::platform::command(smartctl).args(["--scan-open", "-j"]).output() else {
        return Vec::new();
    };
    let Ok(j) = serde_json::from_slice::<Value>(&out.stdout) else {
        return Vec::new();
    };
    j["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let name = d["name"].as_str()?.to_string();
            let t = d["type"].as_str().map(str::to_string);
            Some((name, t))
        })
        .collect()
}

/// macOS: one administrator prompt (osascript) runs smartctl on every disk as root.
#[cfg(target_os = "macos")]
fn mac_admin_read(smartctl: &str) -> Result<Vec<(String, Value)>, &'static str> {
    let q = crate::smartctl::shell_quote(smartctl);
    let script = format!(
        "{q} --scan | while read n x t r; do echo \"@@DEV $n\"; {q} -j -a -d \"$t\" \"$n\"; done"
    );
    let apple = format!(
        "do shell script \"{}\" with administrator privileges",
        script.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let out = crate::platform::command("osascript")
        .args(["-e", &apple])
        .output()
        .map_err(|_| "msg_denied")?;
    // osascript returns the script's output with \r line ends.
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "\n");
    if !text.contains("@@DEV") {
        return Err("msg_denied");
    }
    Ok(split_dev_chunks(&text))
}

/// Windows: disks Windows itself lists (works without administrator rights) and drive letters.
#[cfg(windows)]
fn windows_merge(result: &mut ScanResult) {
    let ps = "$d = @(Get-PhysicalDisk | Select-Object DeviceId,FriendlyName,SerialNumber,Size,MediaType,BusType,FirmwareVersion); \
              $p = @(Get-Partition | Where-Object DriveLetter | Select-Object DiskNumber,DriveLetter); \
              @{disks=$d; parts=$p} | ConvertTo-Json -Depth 3 -Compress";
    let Ok(out) = crate::platform::command("powershell").args(["-NoProfile", "-Command", ps]).output() else {
        return;
    };
    let Ok(j) = serde_json::from_slice::<Value>(&out.stdout) else { return };
    let num = |v: &Value| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()));
    // smartctl names /dev/sda.. and /dev/pdN map to PhysicalDriveN.
    let index_of = |name: &str| -> Option<u64> {
        let n = name.trim_start_matches("/dev/");
        if let Some(rest) = n.strip_prefix("pd") {
            return rest.parse().ok();
        }
        let rest = n.strip_prefix("sd")?;
        let mut idx: u64 = 0;
        for c in rest.chars() {
            if !c.is_ascii_lowercase() {
                return None;
            }
            idx = idx * 26 + (c as u64 - 'a' as u64 + 1);
        }
        idx.checked_sub(1)
    };
    for pd in j["disks"].as_array().into_iter().flatten() {
        let Some(id) = num(&pd["DeviceId"]) else { continue };
        let pos = result.disks.iter().position(|d| index_of(&d.smart_dev) == Some(id));
        let d = match pos {
            Some(i) => &mut result.disks[i],
            None => {
                result.disks.push(disk_for(&format!("/dev/pd{id}"), None));
                result.disks.last_mut().unwrap()
            }
        };
        d.path_label = format!("PhysicalDrive{id}");
        if d.model.is_empty() {
            d.model = pd["FriendlyName"].as_str().unwrap_or("").trim().to_string();
        }
        if d.serial.is_empty() {
            d.serial = pd["SerialNumber"].as_str().unwrap_or("").trim().to_string();
        }
        if d.firmware.is_empty() {
            d.firmware = pd["FirmwareVersion"].as_str().unwrap_or("").trim().to_string();
        }
        if d.size_bytes == 0 {
            d.size_bytes = num(&pd["Size"]).unwrap_or(0);
        }
        if !d.smart_read {
            let media = pd["MediaType"].as_str().map(str::to_string).unwrap_or_else(|| num(&pd["MediaType"]).unwrap_or(0).to_string());
            d.is_ssd = media == "SSD" || media == "4";
            let bus = pd["BusType"].as_str().map(str::to_string).unwrap_or_else(|| num(&pd["BusType"]).unwrap_or(0).to_string());
            d.is_nvme = bus == "NVMe" || bus == "17";
            d.interface = match bus.as_str() {
                "NVMe" | "17" => "NVM Express",
                "SATA" | "11" => "Serial ATA",
                "USB" | "7" => "USB",
                "RAID" | "8" => "RAID",
                "SAS" | "10" => "SAS",
                _ => "—",
            }
            .to_string();
        }
        for p in j["parts"].as_array().into_iter().flatten() {
            if num(&p["DiskNumber"]) == Some(id) {
                // A [char] may come out as text ("C") or as its character code (67).
                let letter = p["DriveLetter"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| num(&p["DriveLetter"]).and_then(|c| char::from_u32(c as u32)).map(String::from));
                if let Some(l) = letter.filter(|l| !l.is_empty() && l != "\0") {
                    let m = format!("{l}:");
                    if !d.mounts.contains(&m) {
                        d.mounts.push(m);
                    }
                }
            }
        }
    }
}

const NVME_ROWS: [(u16, &str, &str); 15] = [
    (0x01, "Advertencia crítica", "critical_warning"),
    (0x02, "Temperatura compuesta", "temperature"),
    (0x03, "Reserva disponible", "available_spare"),
    (0x04, "Umbral de reserva disponible", "available_spare_threshold"),
    (0x05, "Porcentaje usado", "percentage_used"),
    (0x06, "Unidades de datos leídas", "data_units_read"),
    (0x07, "Unidades de datos escritas", "data_units_written"),
    (0x08, "Comandos de lectura del host", "host_reads"),
    (0x09, "Comandos de escritura del host", "host_writes"),
    (0x0A, "Tiempo ocupado del controlador", "controller_busy_time"),
    (0x0B, "Ciclos de encendido", "power_cycles"),
    (0x0C, "Horas encendido", "power_on_hours"),
    (0x0D, "Apagados inseguros", "unsafe_shutdowns"),
    (0x0E, "Errores de integridad de datos", "media_errors"),
    (0x0F, "Entradas del registro de errores", "num_err_log_entries"),
];

/// Fills a disk from `smartctl -j -a` output.
pub fn parse_smart(d: &mut Disk, j: &Value) {
    let s = |p: &str| {
        j.pointer(p)
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let u = |p: &str| j.pointer(p).and_then(Value::as_u64);
    if let Some(v) = s("/model_name") {
        d.model = v;
    }
    if let Some(v) = s("/serial_number") {
        d.serial = v;
    }
    if let Some(v) = s("/firmware_version") {
        d.firmware = v;
    }
    if let Some(v) = u("/user_capacity/bytes") {
        d.size_bytes = v;
    }
    if let Some(r) = u("/rotation_rate") {
        d.is_ssd = r == 0;
        d.rotation_rpm = (r > 0).then_some(r as u32);
    }
    d.power_on_hours = u("/power_on_time/hours").or(d.power_on_hours);
    d.power_on_count = u("/power_cycle_count").or(d.power_on_count);
    if let Some(t) = j.pointer("/temperature/current").and_then(Value::as_i64) {
        d.temperature = Some(t as i32);
    }
    d.features.clear();
    if j.pointer("/smart_support/available").and_then(Value::as_bool) == Some(true) || d.is_nvme {
        d.features.push("S.M.A.R.T.".into());
    }
    let protocol = s("/device/protocol").unwrap_or_default();
    if protocol == "NVMe" {
        d.is_nvme = true;
        d.is_ssd = true;
        parse_nvme(d, j);
    } else {
        parse_ata(d, j);
    }
    if j.pointer("/trim/supported").and_then(Value::as_bool) == Some(true) {
        d.features.push("TRIM".into());
    }
    if j.pointer("/write_cache/enabled").and_then(Value::as_bool) == Some(true) {
        d.features.push("VolatileWriteCache".into());
    }
    d.smart_read = !d.attrs.is_empty();
}

fn parse_ata(d: &mut Disk, j: &Value) {
    let s = |p: &str| j.pointer(p).and_then(Value::as_str).map(str::to_string);
    if d.interface == "—" || d.interface.is_empty() {
        d.interface = "Serial ATA".into();
    }
    let std = [s("/ata_version/string"), s("/sata_version/string")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" | ");
    if !std.is_empty() {
        d.standard = std;
    }
    let sata = |p: &str| {
        s(p).map(|v| match v.split_whitespace().next().unwrap_or("") {
            "6.0" => "SATA/600".to_string(),
            "3.0" => "SATA/300".to_string(),
            "1.5" => "SATA/150".to_string(),
            _ => v,
        })
    };
    if let (Some(c), Some(m)) = (sata("/interface_speed/current/string"), sata("/interface_speed/max/string")) {
        d.transfer_mode = format!("{c} | {m}");
    } else if let Some(m) = sata("/interface_speed/max/string") {
        d.transfer_mode = format!("— | {m}");
    }
    if j.pointer("/user_capacity/blocks").and_then(Value::as_u64).unwrap_or(0) > (1 << 28) {
        d.features.push("48bit LBA".into());
    }
    if j.pointer("/ata_apm/enabled").and_then(Value::as_bool) == Some(true) {
        d.features.push("APM".into());
    }
    d.attrs.clear();
    let mut reads = None;
    let mut writes = None;
    for a in j.pointer("/ata_smart_attributes/table").and_then(Value::as_array).into_iter().flatten() {
        let id = a["id"].as_u64().unwrap_or(0) as u16;
        let raw_name = a["name"].as_str().unwrap_or("");
        let raw = a["raw"]["value"].as_u64().unwrap_or(0);
        let attr = Attr {
            id,
            name: raw_name.to_string(),
            current: a["value"].as_u64().map(|v| v as u32),
            worst: a["worst"].as_u64().map(|v| v as u32),
            thresh: a["thresh"].as_u64().map(|v| v as u32),
            raw,
            status: Status::Good,
        };
        let unit = if raw_name.contains("32MiB") {
            Some(32u64 << 20)
        } else if raw_name.contains("GiB") {
            Some(1u64 << 30)
        } else if raw_name.contains("LBAs") {
            Some(512)
        } else {
            None
        };
        if let Some(unit) = unit {
            let gb = (raw as u128 * unit as u128 / 1_000_000_000) as u64;
            if id == 0xF1 || raw_name.contains("Write") {
                writes.get_or_insert(gb);
            } else if id == 0xF2 || raw_name.contains("Read") {
                reads.get_or_insert(gb);
            }
        }
        d.attrs.push(attr);
    }
    d.host_reads_gb = reads;
    d.host_writes_gb = writes;
    // Remaining life of an SSD: first known life attribute present (estimated choice).
    if d.is_ssd {
        let by_name = ["Percent_Lifetime_Remain", "SSD_Life_Left", "Media_Wearout_Indicator", "Remaining_Lifetime_Perc", "Wear_Leveling_Count"];
        let named = j
            .pointer("/ata_smart_attributes/table")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|a| by_name.iter().any(|n| a["name"].as_str().unwrap_or("").contains(n)))
            .and_then(|a| a["id"].as_u64())
            .map(|i| i as u16);
        let id = named.or_else(|| {
            [0xE7u16, 0xA9, 0xCA, 0xB1, 0xE9, 0xAD]
                .into_iter()
                .find(|i| d.attrs.iter().any(|a| a.id == *i))
        });
        if let Some(id) = id {
            if let Some(cur) = d.attrs.iter().find(|a| a.id == id).and_then(|a| a.current) {
                d.life = Some((cur as i32).clamp(0, 100));
                d.life_attr = Some(id);
            }
        }
    }
}

fn parse_nvme(d: &mut Disk, j: &Value) {
    d.interface = "NVM Express".into();
    if let Some(v) = j.pointer("/nvme_version/string").and_then(Value::as_str) {
        d.standard = format!("NVM Express {v}");
    }
    let log = &j["nvme_smart_health_information_log"];
    if !log.is_object() {
        return;
    }
    d.attrs.clear();
    for (id, name, key) in NVME_ROWS {
        let mut raw = log[key].as_u64().unwrap_or(0);
        if key == "temperature" && raw > 0 {
            raw += 273; // raw value in Kelvin, as the drive reports it
        }
        d.attrs.push(Attr { id, name: name.into(), raw, status: Status::Good, ..Default::default() });
    }
    d.nvme_critical = log["critical_warning"].as_u64();
    d.nvme_spare = log["available_spare"].as_i64();
    d.nvme_spare_thresh = log["available_spare_threshold"].as_i64();
    if let Some(used) = log["percentage_used"].as_i64() {
        d.life = Some((100 - used as i32).clamp(0, 100));
    }
    // One data unit = 1000 sectors of 512 bytes (NVMe spec).
    d.host_reads_gb = log["data_units_read"].as_u64().map(|u| u * 512_000 / 1_000_000_000);
    d.host_writes_gb = log["data_units_written"].as_u64().map(|u| u * 512_000 / 1_000_000_000);
    d.power_on_hours = log["power_on_hours"].as_u64().or(d.power_on_hours);
    d.power_on_count = log["power_cycles"].as_u64().or(d.power_on_count);
    if let Some(t) = log["temperature"].as_i64() {
        d.temperature = Some(t as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::{evaluate, Thresholds};

    #[test]
    fn ata_hdd_caution_on_reallocated() {
        let j: Value = serde_json::from_str(r#"{"device":{"protocol":"ATA"},"model_name":"HDD","rotation_rate":5400,
          "ata_smart_attributes":{"table":[
            {"id":5,"name":"Reallocated_Sector_Ct","value":100,"worst":100,"thresh":10,"raw":{"value":8}},
            {"id":241,"name":"Total_LBAs_Written","value":100,"worst":100,"thresh":0,"raw":{"value":2000000000}}]}}"#).unwrap();
        let mut d = Disk::default();
        parse_smart(&mut d, &j);
        evaluate(&mut d, &Thresholds::defaults(false));
        assert_eq!(d.health, Status::Caution);
        assert_eq!(d.host_writes_gb, Some(1024));
    }

    #[test]
    fn ata_bad_when_below_threshold() {
        let j: Value = serde_json::from_str(r#"{"device":{"protocol":"ATA"},"rotation_rate":7200,
          "ata_smart_attributes":{"table":[{"id":1,"name":"Raw_Read_Error_Rate","value":5,"worst":5,"thresh":6,"raw":{"value":0}}]}}"#).unwrap();
        let mut d = Disk::default();
        parse_smart(&mut d, &j);
        evaluate(&mut d, &Thresholds::defaults(false));
        assert_eq!(d.health, Status::Bad);
    }

    #[test]
    fn nvme_rules() {
        let mk = |spare: i64, used: i64, crit: i64| -> Disk {
            let j: Value = serde_json::from_str(&format!(r#"{{"device":{{"protocol":"NVMe"}},"nvme_smart_health_information_log":
              {{"critical_warning":{crit},"temperature":40,"available_spare":{spare},"available_spare_threshold":10,"percentage_used":{used},
                "data_units_written":2000000,"power_on_hours":100,"power_cycles":5}}}}"#)).unwrap();
            let mut d = Disk::default();
            parse_smart(&mut d, &j);
            evaluate(&mut d, &Thresholds::defaults(true));
            d
        };
        let ok = mk(100, 3, 0);
        assert_eq!(ok.health, Status::Good);
        assert_eq!(ok.life, Some(97));
        assert_eq!(ok.host_writes_gb, Some(1024));
        assert_eq!(mk(10, 3, 0).health, Status::Caution);
        assert_eq!(mk(5, 3, 0).health, Status::Bad);
        assert_eq!(mk(100, 95, 0).health, Status::Caution);
        assert_eq!(mk(100, 3, 1).health, Status::Bad);
    }
}
