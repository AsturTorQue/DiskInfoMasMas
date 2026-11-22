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
    /// File system ("ext4", "NTFS", "APFS"); "crypto_LUKS > btrfs" when layered.
    pub fs: String,
    pub size: u64,
    pub used: Option<u64>,
    pub free: Option<u64>,
    pub mounts: Vec<String>,
    /// Partition type ("EFI System", "Microsoft basic data", "Linux filesystem").
    pub kind: String,
    pub uuid: String,
    /// Encryption in use ("LUKS", "BitLocker", "FileVault"), empty when none was found.
    pub encryption: String,
    /// Mounted read-only (often a sign the file system found errors).
    pub read_only: bool,
    /// Share of file entries (inodes) used, 0..=1, on file systems that have a fixed number.
    pub inodes_used: Option<f32>,
    /// The system's own verdict on the volume ("Healthy", "Warning"...), when it gives one.
    pub health: String,
    /// What lives inside: btrfs subvolumes, ZFS datasets, LUKS/LVM/RAID devices.
    pub subs: Vec<SubVolume>,
}

/// Something inside a partition: a btrfs subvolume, a ZFS dataset, or a device stacked on it
/// (encrypted LUKS volume, LVM logical volume, RAID array).
#[derive(Clone, Debug, Default)]
pub struct SubVolume {
    /// "/home", "@snapshots", "luks-1234", "vg0-root", "tank/data"...
    pub name: String,
    /// i18n key of its kind: sub_btrfs, sub_zfs, sub_crypt, sub_lvm, sub_raid, sub_other.
    pub kind: &'static str,
    pub fs: String,
    /// Its own size, when it has one (subvolumes share the partition's space).
    pub size: Option<u64>,
    pub used: Option<u64>,
    pub free: Option<u64>,
    pub mounts: Vec<String>,
    /// btrfs subvolume id, when known.
    pub id: String,
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
        if layers.iter().any(|l| l == "crypto_LUKS") {
            p.encryption = "LUKS".into();
        } else if layers.iter().any(|l| l.eq_ignore_ascii_case("BitLocker")) {
            p.encryption = "BitLocker".into();
        }
        p.read_only = cur["ro"].as_bool().unwrap_or(false) || c["ro"].as_bool().unwrap_or(false);
        p.fs = layers.join(" > ");
        // Devices stacked on the partition (LUKS, LVM, RAID), at any depth.
        let mut names = vec![p.name.clone()];
        collect_stacked(c, &mut p.subs, &mut names);
        // btrfs subvolumes and ZFS datasets mounted from it or from a device on it.
        let pool = (layers.iter().any(|l| l == "zfs_member")).then(|| p.label.clone());
        add_mounted_subvolumes(&mut p, &names, pool.as_deref());
        d.partitions.push(p);
    }
    linux_mount_health(&mut d.partitions);
}

/// Adds every device below `node` (encrypted volumes, LVM, RAID) and remembers their names.
#[cfg(target_os = "linux")]
fn collect_stacked(node: &Value, out: &mut Vec<SubVolume>, names: &mut Vec<String>) {
    let s = |v: &Value| v.as_str().map(|s| s.trim().to_string()).unwrap_or_default();
    for c in node["children"].as_array().into_iter().flatten() {
        let ty = s(&c["type"]);
        let kind = match ty.as_str() {
            "crypt" => "sub_crypt",
            "lvm" => "sub_lvm",
            t if t.starts_with("raid") || t == "md" => "sub_raid",
            _ => "sub_other",
        };
        let name = s(&c["name"]);
        names.push(name.clone());
        out.push(SubVolume {
            name,
            kind,
            fs: s(&c["fstype"]),
            size: c["size"].as_u64(),
            used: c["fsused"].as_u64(),
            free: c["fsavail"].as_u64(),
            mounts: c["mountpoints"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| m.as_str().filter(|m| !m.is_empty()).map(str::to_string))
                .collect(),
            id: String::new(),
        });
        collect_stacked(c, out, names);
    }
}

/// One line of /proc/self/mountinfo, the parts we use.
#[cfg(target_os = "linux")]
struct MountLine {
    mount_point: String,
    fstype: String,
    source: String,
    super_opts: String,
}

#[cfg(target_os = "linux")]
fn mountinfo() -> Vec<MountLine> {
    // Spaces and the like come escaped as \040 (octal).
    let unescape = |s: &str| {
        let mut out = String::new();
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(u8::is_ascii_digit) {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 4], 8) {
                    out.push(v as char);
                    i += 4;
                    continue;
                }
            }
            out.push(b[i] as char);
            i += 1;
        }
        out
    };
    let text = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let (left, right) = l.split_once(" - ")?;
            let lf: Vec<&str> = left.split_whitespace().collect();
            let rf: Vec<&str> = right.split_whitespace().collect();
            Some(MountLine {
                mount_point: unescape(lf.get(4)?),
                fstype: rf.first()?.to_string(),
                source: unescape(rf.get(1)?),
                super_opts: rf.get(2).unwrap_or(&"").to_string(),
            })
        })
        .collect()
}

/// btrfs subvolumes and ZFS datasets that are mounted, grouped with all their mount points.
/// (Unmounted subvolumes need administrator rights to list, so they are not shown.)
#[cfg(target_os = "linux")]
fn add_mounted_subvolumes(p: &mut Partition, names: &[String], zfs_pool: Option<&str>) {
    let lines = mountinfo();
    let base = |src: &str| src.rsplit('/').next().unwrap_or(src).to_string();
    let mut found: Vec<SubVolume> = Vec::new();
    for m in &lines {
        let (name, kind, id) = if m.fstype == "btrfs" && names.contains(&base(&m.source)) {
            let opt = |k: &str| {
                m.super_opts.split(',').find_map(|o| o.strip_prefix(k)).map(str::to_string)
            };
            let subvol = opt("subvol=").unwrap_or_else(|| "/".into());
            (subvol, "sub_btrfs", opt("subvolid=").unwrap_or_default())
        } else if m.fstype == "zfs" && zfs_pool.is_some_and(|pool| !pool.is_empty() && (m.source == pool || m.source.starts_with(&format!("{pool}/")))) {
            (m.source.clone(), "sub_zfs", String::new())
        } else {
            continue;
        };
        match found.iter_mut().find(|s| s.name == name && s.kind == kind) {
            Some(s) => {
                if !s.mounts.contains(&m.mount_point) {
                    s.mounts.push(m.mount_point.clone());
                }
            }
            None => found.push(SubVolume {
                name,
                kind,
                fs: m.fstype.clone(),
                mounts: vec![m.mount_point.clone()],
                id,
                ..Default::default()
            }),
        }
    }
    // Shortest mount point first (the "main" one), and subvolumes in path order.
    for s in &mut found {
        s.mounts.sort_by_key(|m| (m.len(), m.clone()));
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    p.subs.extend(found);
}

/// Read-only mounts (from /proc/mounts) and inode use (from `df -Pi`) of mounted partitions.
#[cfg(target_os = "linux")]
fn linux_mount_health(parts: &mut [Partition]) {
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let ro: Vec<String> = mounts
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let opts = f.get(3)?;
            opts.split(',').any(|o| o == "ro").then(|| f[1].replace("\\040", " "))
        })
        .collect();
    let targets: Vec<String> = parts.iter().flat_map(|p| p.mounts.iter().cloned()).collect();
    if targets.is_empty() {
        return;
    }
    let mut inodes = std::collections::HashMap::new();
    if let Ok(o) = crate::platform::command("df").arg("-Pi").args(&targets).output() {
        for line in String::from_utf8_lossy(&o.stdout).lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 6 {
                continue;
            }
            // Filesystem Inodes IUsed IFree IUse% Mounted-on (the mount may contain spaces).
            let total: u64 = f[1].parse().unwrap_or(0);
            let used: u64 = f[2].parse().unwrap_or(0);
            if total > 0 {
                inodes.insert(f[5..].join(" "), used as f32 / total as f32);
            }
        }
    }
    for p in parts.iter_mut() {
        // Immutable systems (Bazzite, Silverblue...) mount parts of a writable file system
        // read-only on purpose, so only flag it when every mount of it is read-only.
        if !p.mounts.is_empty() && p.mounts.iter().all(|m| ro.contains(m)) {
            p.read_only = true;
        } else if !p.mounts.is_empty() {
            p.read_only = false;
        }
        p.inodes_used = p.mounts.iter().find_map(|m| inodes.get(m).copied());
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
VolumeSize=$(if($v){$v.Size}); AccessPaths=@($_.AccessPaths); Health=$(if($v){[string]$v.HealthStatus}); \
BitLocker=$(if($_.DriveLetter){ try { (New-Object -ComObject Shell.Application).NameSpace(\"$($_.DriveLetter):\").Self.ExtendedProperty('System.Volume.BitLockerProtection') } catch { $null } }) } }); \
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
        part.health = s(&p["Health"]);
        // System.Volume.BitLockerProtection: 1 on, 3 encrypting, 5 locked, 6 unlocked; 2 off.
        if matches!(num(&p["BitLocker"]), Some(1 | 3 | 5 | 6)) {
            part.encryption = "BitLocker".into();
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
            // FileVault and write access, per volume.
            if let Ok(o) = crate::platform::command("diskutil").args(["info", "-plist", &part.name]).output() {
                if let Some(info) = plist::Value::from_reader_xml(&o.stdout[..]).ok().and_then(|v| v.into_dictionary()) {
                    let flag = |k: &str| info.get(k).and_then(|v| v.as_boolean());
                    if flag("FileVault") == Some(true) || flag("Encryption") == Some(true) {
                        part.encryption = "FileVault".into();
                    }
                    part.read_only = flag("WritableVolume") == Some(false) && !mount.is_empty();
                }
            }
            if !mount.is_empty() {
                part.mounts.push(mount.clone());
                if let Some((used, free)) = df_usage(&mount) {
                    part.used = Some(used);
                    part.free = Some(free);
                }
            }
            if part.fs == "Apple_APFS" {
                apfs_volumes(&mut part, all);
            }
            d.partitions.push(part);
        }
    }
    // The disk's own list of mount points (shown under its tab and in its details).
    d.mounts = d.partitions.iter().flat_map(|p| p.mounts.iter().cloned()).filter(|m| !m.starts_with("/System/Volumes/") || m == "/System/Volumes/Data").collect();
}

/// An APFS partition holds a container whose volumes ("Macintosh HD", "Data"...) live on a
/// separate synthesized disk (disk3s1...): list them under the partition, with their mount points.
#[cfg(target_os = "macos")]
fn apfs_volumes(part: &mut Partition, all: &[plist::Value]) {
    let text = |dict: &plist::Dictionary, k: &str| dict.get(k).and_then(|v| v.as_string()).unwrap_or("").to_string();
    let Some(container) = all.iter().filter_map(|v| v.as_dictionary()).find(|c| {
        c.get("APFSPhysicalStores").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|s| s.as_dictionary()).any(|s| text(s, "DeviceIdentifier") == part.name)
    }) else {
        return;
    };
    part.fs = "APFS".into();
    for v in container.get("APFSVolumes").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_dictionary()) {
        let mount = text(v, "MountPoint");
        let in_use = v.get("CapacityInUse").and_then(|x| x.as_unsigned_integer());
        if !mount.is_empty() && !part.mounts.contains(&mount) {
            part.mounts.push(mount.clone());
        }
        part.subs.push(SubVolume {
            name: { let n = text(v, "VolumeName"); if n.is_empty() { text(v, "DeviceIdentifier") } else { n } },
            kind: "sub_apfs",
            fs: "APFS".into(),
            size: in_use,
            used: in_use,
            free: None,
            mounts: if mount.is_empty() { vec![] } else { vec![mount] },
            id: text(v, "DeviceIdentifier"),
        });
    }
    // Space used and free, from the volume people keep their files on.
    let main = part.mounts.iter().find(|m| m.as_str() == "/System/Volumes/Data").or_else(|| part.mounts.first()).cloned();
    if let Some((used, free)) = main.and_then(|m| df_usage(&m)) {
        // Every volume shares the container's free space.
        let _ = used;
        part.free = Some(free);
        part.used = Some(part.size.saturating_sub(free));
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
