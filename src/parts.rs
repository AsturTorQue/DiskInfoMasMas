//! Partitions of each disk and extra facts about it, from what the system tells any user:
//! lsblk on Linux, Get-Partition/Get-Volume/Get-Disk on Windows, diskutil and df on macOS.

use crate::disks::Disk;
#[allow(unused_imports)]
use serde_json::Value;

#[derive(Clone, Debug, Default)]
pub struct Partition {
    /// "sda1", "C:", "disk0s2"...
    pub name: String,
    pub label: String,
    /// File system ("ext4", "NTFS", "APFS"); "crypto_LUKS → btrfs" when layered.
    pub fs: String,
    pub size: u64,
    pub used: Option<u64>,
    pub free: Option<u64>,
    pub mounts: Vec<String>,
    /// Partition type ("EFI System", "Microsoft basic data", "Linux filesystem").
    pub kind: String,
    pub uuid: String,
}

impl Partition {
    /// Share of the partition in use, 0..=1, when known.
    pub fn used_fraction(&self) -> Option<f32> {
        let total = match (self.used, self.free) {
            (Some(u), Some(f)) if u + f > 0 => u + f,
            _ => return None,
        };
        Some(self.used? as f32 / total as f32)
    }
}

/// Sets a detail (an i18n key and its value), replacing an earlier value for the same key.
pub fn set_detail(d: &mut Disk, key: &'static str, value: impl Into<String>) {
    let value = value.into();
    if value.trim().is_empty() {
        return;
    }
    match d.details.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => d.details.push((key, value)),
    }
}

/// Bytes not covered by any partition.
pub fn unallocated(d: &Disk) -> u64 {
    let used: u64 = d.partitions.iter().map(|p| p.size).sum();
    d.size_bytes.saturating_sub(used)
}

// ---------------------------------------------------------------- Linux

/// Fills partitions and system details of one lsblk disk node.
#[cfg(target_os = "linux")]
pub fn from_lsblk(d: &mut Disk, node: &Value) {
    let s = |v: &Value| v.as_str().map(|s| s.trim().to_string()).unwrap_or_default();
    set_detail(d, "d_path", format!("/dev/{}", d.dev));
    set_detail(d, "d_part_table", table_name(&s(&node["pttype"])));
    set_detail(d, "d_wwn", s(&node["wwn"]));
    set_detail(d, "d_vendor", s(&node["vendor"]));
    if let (Some(l), Some(p)) = (node["log-sec"].as_u64(), node["phy-sec"].as_u64()) {
        set_detail(d, "d_sector", format!("{l} / {p} B"));
    }
    if let Some(rm) = node["rm"].as_bool() {
        set_detail(d, "d_removable", if rm { "yes" } else { "no" });
    }
    set_detail(d, "d_scheduler", s(&node["sched"]));
    set_detail(d, "d_hctl", s(&node["hctl"]));
    d.partitions.clear();
    for c in node["children"].as_array().into_iter().flatten() {
        if c["type"].as_str() != Some("part") {
            continue;
        }
        let mut p = Partition {
            name: s(&c["name"]),
            label: [s(&c["label"]), s(&c["partlabel"])].into_iter().find(|v| !v.is_empty()).unwrap_or_default(),
            size: c["size"].as_u64().unwrap_or(0),
            kind: s(&c["parttypename"]),
            uuid: s(&c["uuid"]),
            ..Default::default()
        };
        // Follow layers (encryption, LVM) down to the file system that is mounted.
        let mut layers = Vec::new();
        let mut cur = c;
        loop {
            let fs = s(&cur["fstype"]);
            if !fs.is_empty() {
                layers.push(fs);
            }
            for m in cur["mountpoints"].as_array().into_iter().flatten() {
                if let Some(m) = m.as_str().filter(|m| !m.is_empty() && !p.mounts.iter().any(|x| x == m)) {
                    p.mounts.push(m.to_string());
                }
            }
            if let (Some(u), Some(f)) = (cur["fsused"].as_u64(), cur["fsavail"].as_u64()) {
                p.used = Some(u);
                p.free = Some(f);
            }
            if p.label.is_empty() {
                p.label = s(&cur["label"]);
            }
            match cur["children"].as_array().and_then(|a| a.first()) {
                Some(next) => cur = next,
                None => break,
            }
        }
        p.fs = layers.join(" → ");
        d.partitions.push(p);
    }
}

#[cfg(target_os = "linux")]
fn table_name(t: &str) -> String {
    match t {
        "gpt" => "GPT".into(),
        "dos" => "MBR".into(),
        other => other.to_uppercase(),
    }
}

// ---------------------------------------------------------------- Windows

/// PowerShell that prints, as JSON, each partition with its volume and each disk's facts.
#[cfg(windows)]
pub const WINDOWS_PS: &str = "\
$parts = @(Get-Partition | ForEach-Object { $v = $null; try { $v = $_ | Get-Volume -ErrorAction Stop } catch {}; \
[pscustomobject]@{ DiskNumber=$_.DiskNumber; PartitionNumber=$_.PartitionNumber; DriveLetter=[string]$_.DriveLetter; \
Size=$_.Size; Type=[string]$_.Type; GptType=[string]$_.GptType; IsBoot=$_.IsBoot; IsSystem=$_.IsSystem; \
FileSystem=$(if($v){$v.FileSystem}); Label=$(if($v){$v.FileSystemLabel}); Remaining=$(if($v){$v.SizeRemaining}); \
VolumeSize=$(if($v){$v.Size}); AccessPaths=@($_.AccessPaths) } }); \
$disks = @(Get-Disk | Select-Object Number,PartitionStyle,@{n='HealthStatus';e={[string]$_.HealthStatus}},@{n='OperationalStatus';e={[string]$_.OperationalStatus}},IsBoot,IsSystem,LogicalSectorSize,PhysicalSectorSize,UniqueId,Manufacturer,Location); \
@{parts=$parts; disks=$disks} | ConvertTo-Json -Depth 4 -Compress";

/// Applies the output of `WINDOWS_PS` to the disk with Windows number `id`.
#[cfg(windows)]
pub fn apply_windows(d: &mut Disk, id: u64, j: &Value) {
    let num = |v: &Value| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()));
    let s = |v: &Value| match v {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    };
    let yes_no = |v: &Value| v.as_bool().map(|b| if b { "yes" } else { "no" });
    set_detail(d, "d_path", format!("\\\\.\\PhysicalDrive{id}"));
    for disk in j["disks"].as_array().into_iter().flatten() {
        if num(&disk["Number"]) != Some(id) {
            continue;
        }
        let style = match s(&disk["PartitionStyle"]).as_str() {
            "1" => "MBR".to_string(),
            "2" => "GPT".to_string(),
            "0" => "RAW".to_string(),
            other => other.to_string(),
        };
        set_detail(d, "d_part_table", style);
        set_detail(d, "d_os_health", s(&disk["HealthStatus"]));
        set_detail(d, "d_os_state", s(&disk["OperationalStatus"]));
        if let Some(b) = yes_no(&disk["IsBoot"]) {
            set_detail(d, "d_boot", b);
        }
        if let Some(b) = yes_no(&disk["IsSystem"]) {
            set_detail(d, "d_system", b);
        }
        if let (Some(l), Some(p)) = (num(&disk["LogicalSectorSize"]), num(&disk["PhysicalSectorSize"])) {
            set_detail(d, "d_sector", format!("{l} / {p} B"));
        }
        set_detail(d, "d_vendor", s(&disk["Manufacturer"]));
        set_detail(d, "d_location", s(&disk["Location"]));
    }
    d.partitions.clear();
    for p in j["parts"].as_array().into_iter().flatten() {
        if num(&p["DiskNumber"]) != Some(id) {
            continue;
        }
        let letter = s(&p["DriveLetter"]);
        let letter = letter.trim_matches('\0').to_string();
        let mut part = Partition {
            name: if letter.is_empty() {
                format!("#{}", s(&p["PartitionNumber"]))
            } else {
                format!("{letter}:")
            },
            label: s(&p["Label"]),
            fs: s(&p["FileSystem"]),
            size: num(&p["Size"]).unwrap_or(0),
            kind: s(&p["Type"]),
            ..Default::default()
        };
        if let (Some(total), Some(rem)) = (num(&p["VolumeSize"]), num(&p["Remaining"])) {
            part.used = Some(total.saturating_sub(rem));
            part.free = Some(rem);
        }
        for a in p["AccessPaths"].as_array().into_iter().flatten() {
            if let Some(a) = a.as_str().filter(|a| !a.starts_with("\\\\?\\")) {
                part.mounts.push(a.to_string());
            }
        }
        d.partitions.push(part);
    }
}

// ---------------------------------------------------------------- macOS

/// Partitions of a whole disk from `diskutil list -plist`, plus usage from `df`.
#[cfg(target_os = "macos")]
pub fn from_diskutil(d: &mut Disk) {
    let id = d.dev.trim_start_matches("/dev/").to_string();
    set_detail(d, "d_path", format!("/dev/{id}"));
    let Ok(out) = crate::platform::command("diskutil").args(["list", "-plist", &id]).output() else {
        return;
    };
    let Ok(root) = plist::Value::from_reader_xml(&out.stdout[..]) else { return };
    let Some(all) = root.as_dictionary().and_then(|r| r.get("AllDisksAndPartitions")).and_then(|v| v.as_array()) else {
        return;
    };
    let text = |dict: &plist::Dictionary, k: &str| dict.get(k).and_then(|v| v.as_string()).unwrap_or("").to_string();
    let int = |dict: &plist::Dictionary, k: &str| dict.get(k).and_then(|v| v.as_unsigned_integer()).unwrap_or(0);
    d.partitions.clear();
    for disk in all.iter().filter_map(|v| v.as_dictionary()) {
        if text(disk, "DeviceIdentifier") != id {
            continue;
        }
        set_detail(d, "d_part_table", text(disk, "Content").replace("GUID_partition_scheme", "GPT").replace("FDisk_partition_scheme", "MBR"));
        for p in disk.get("Partitions").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_dictionary()) {
            let mut part = Partition {
                name: text(p, "DeviceIdentifier"),
                label: text(p, "VolumeName"),
                fs: text(p, "Content"),
                size: int(p, "Size"),
                kind: text(p, "Content"),
                uuid: text(p, "VolumeUUID"),
                ..Default::default()
            };
            let mount = text(p, "MountPoint");
            if !mount.is_empty() {
                part.mounts.push(mount.clone());
                if let Some((used, free)) = df_usage(&mount) {
                    part.used = Some(used);
                    part.free = Some(free);
                }
            }
            d.partitions.push(part);
        }
    }
}

/// (used, free) bytes of a mounted file system, from `df -Pk`.
#[cfg(target_os = "macos")]
fn df_usage(mount: &str) -> Option<(u64, u64)> {
    let out = crate::platform::command("df").args(["-Pk", mount]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().nth(1)?;
    let f: Vec<&str> = line.split_whitespace().collect();
    let used: u64 = f.get(2)?.parse().ok()?;
    let free: u64 = f.get(3)?.parse().ok()?;
    Some((used * 1024, free * 1024))
}
