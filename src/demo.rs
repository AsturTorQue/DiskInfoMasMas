//! Two sample disks with S.M.A.R.T. data, for `--demo` (no real disk is read).

use crate::disks::{parse_smart, Disk, ScanResult};
use crate::parts::{set_detail, Partition};
use serde_json::json;

pub fn demo_scan() -> ScanResult {
    let hdd = json!({
        "device": {"protocol": "ATA"},
        "model_name": "Sample HDD 4TB", "serial_number": "DEMO0001", "firmware_version": "0001",
        "user_capacity": {"bytes": 4000787030016u64, "blocks": 7814037168u64},
        "rotation_rate": 5400, "ata_version": {"string": "ACS-3"}, "sata_version": {"string": "SATA 3.1"},
        "interface_speed": {"max": {"string": "6.0 Gb/s"}, "current": {"string": "6.0 Gb/s"}},
        "smart_support": {"available": true}, "power_on_time": {"hours": 21034}, "power_cycle_count": 812,
        "temperature": {"current": 38},
        "ata_smart_attributes": {"table": [
            {"id": 1, "name": "Raw_Read_Error_Rate", "value": 83, "worst": 64, "thresh": 6, "raw": {"value": 197316232u64}},
            {"id": 3, "name": "Spin_Up_Time", "value": 96, "worst": 96, "thresh": 0, "raw": {"value": 0}},
            {"id": 4, "name": "Start_Stop_Count", "value": 100, "worst": 100, "thresh": 20, "raw": {"value": 830}},
            {"id": 5, "name": "Reallocated_Sector_Ct", "value": 100, "worst": 100, "thresh": 10, "raw": {"value": 8}},
            {"id": 7, "name": "Seek_Error_Rate", "value": 88, "worst": 60, "thresh": 45, "raw": {"value": 658172342u64}},
            {"id": 9, "name": "Power_On_Hours", "value": 77, "worst": 77, "thresh": 0, "raw": {"value": 21034}},
            {"id": 12, "name": "Power_Cycle_Count", "value": 100, "worst": 100, "thresh": 20, "raw": {"value": 812}},
            {"id": 194, "name": "Temperature_Celsius", "value": 38, "worst": 50, "thresh": 0, "raw": {"value": 38}},
            {"id": 197, "name": "Current_Pending_Sector", "value": 100, "worst": 100, "thresh": 0, "raw": {"value": 0}},
            {"id": 198, "name": "Offline_Uncorrectable", "value": 100, "worst": 100, "thresh": 0, "raw": {"value": 0}},
            {"id": 199, "name": "UDMA_CRC_Error_Count", "value": 200, "worst": 200, "thresh": 0, "raw": {"value": 0}},
            {"id": 241, "name": "Total_LBAs_Written", "value": 100, "worst": 253, "thresh": 0, "raw": {"value": 40210931920u64}},
            {"id": 242, "name": "Total_LBAs_Read", "value": 100, "worst": 253, "thresh": 0, "raw": {"value": 98123456789u64}}
        ]}
    });
    let nvme = json!({
        "device": {"protocol": "NVMe"}, "model_name": "Sample NVMe 1TB", "serial_number": "DEMO0002",
        "firmware_version": "1.0", "user_capacity": {"bytes": 1000204886016u64}, "nvme_version": {"string": "1.4"},
        "nvme_smart_health_information_log": {
            "critical_warning": 0, "temperature": 41, "available_spare": 100, "available_spare_threshold": 10,
            "percentage_used": 3, "data_units_read": 31205678, "data_units_written": 29876543, "host_reads": 512345678,
            "host_writes": 498765432, "controller_busy_time": 1234, "power_cycles": 1520, "power_on_hours": 4120,
            "unsafe_shutdowns": 61, "media_errors": 0, "num_err_log_entries": 0
        }
    });
    let mut a = Disk {
        dev: "sda".into(),
        interface: "Serial ATA".into(),
        mounts: vec!["/mnt/datos/una carpeta con un nombre largo".into()],
        ..Default::default()
    };
    parse_smart(&mut a, &hdd);
    let mut b = Disk {
        dev: "nvme0n1".into(),
        is_nvme: true,
        transfer_mode: "PCIe 3.0 x4 | PCIe 4.0 x4".into(),
        mounts: vec!["/".into(), "/home".into(), "/boot/efi".into()],
        ..Default::default()
    };
    parse_smart(&mut b, &nvme);

    let gb = 1_000_000_000u64;
    let part = |name: &str, label: &str, fs: &str, size: u64, used: Option<u64>, mount: &str, kind: &str| Partition {
        name: name.into(),
        label: label.into(),
        fs: fs.into(),
        size,
        used,
        free: used.map(|u| size.saturating_sub(u)),
        mounts: if mount.is_empty() { vec![] } else { vec![mount.into()] },
        kind: kind.into(),
        uuid: String::new(),
    };
    a.partitions = vec![
        part("sda1", "", "", 134_217_728, None, "", "Microsoft reserved"),
        part("sda2", "Datos", "ntfs", 4_000 * gb - 134_217_728 - 600_000_000, Some(1_946 * gb), "/mnt/datos/una carpeta con un nombre largo", "Microsoft basic data"),
    ];
    set_detail(&mut a, "d_path", "/dev/sda");
    set_detail(&mut a, "d_part_table", "GPT");
    set_detail(&mut a, "d_scheduler", "bfq");
    set_detail(&mut a, "d_removable", "no");
    b.partitions = vec![
        part("nvme0n1p1", "EFI", "vfat", 600_000_000, Some(20_000_000), "/boot/efi", "EFI System"),
        part("nvme0n1p2", "boot", "ext4", 2 * gb, Some(450_000_000), "/boot", "Linux extended boot"),
        part("nvme0n1p3", "fedora", "crypto_LUKS → btrfs", 997 * gb, Some(870 * gb), "/", "Linux filesystem"),
    ];
    set_detail(&mut b, "d_path", "/dev/nvme0n1");
    set_detail(&mut b, "d_part_table", "GPT");
    ScanResult { disks: vec![a, b], needs_admin: false, message: None, passwordless: false }
}
