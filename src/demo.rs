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
        "ata_sct_status": {"temperature": {"current": 38, "power_cycle_min": 31, "power_cycle_max": 41, "lifetime_min": 18, "lifetime_max": 52, "op_limit_max": 60}},
        "ata_smart_data": {"self_test": {"status": {"value": 0, "string": "completed without error"}, "polling_minutes": {"short": 1, "extended": 512}}},
        "ata_smart_self_test_log": {"standard": {"table": [
            {"type": {"string": "Short offline"}, "status": {"string": "Completed without error"}, "lifetime_hours": 21010},
            {"type": {"string": "Extended offline"}, "status": {"string": "Completed without error"}, "lifetime_hours": 20500}
        ]}},
        "sata_phy_event_counters": {"table": [
            {"id": 1, "name": "Command failed due to ICRC error", "value": 0},
            {"id": 10, "name": "Device-to-host register FISes sent due to a COMRESET", "value": 3}
        ]},
        "write_cache": {"enabled": true}, "read_lookahead": {"enabled": true},
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
            "unsafe_shutdowns": 61, "media_errors": 0, "num_err_log_entries": 0,
            "temperature_sensors": [41, 52]
        },
        "nvme_composite_temperature_threshold": {"warning": 82, "critical": 85},
        "nvme_power_states": [
            {"non_operational_state": false, "max_power": {"value": 750, "scale": 2, "units_per_watt": 100}},
            {"non_operational_state": false, "max_power": {"value": 580, "scale": 2, "units_per_watt": 100}},
            {"non_operational_state": true, "max_power": {"value": 5, "scale": 2, "units_per_watt": 100}}
        ],
        "nvme_self_test_log": {"current_self_test_operation": {"value": 0, "string": "No self-test in progress"},
            "table": [{"self_test_code": {"string": "Short"}, "self_test_result": {"string": "Completed without error"}, "power_on_hours": 4100}]}
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
        encryption: if fs.starts_with("crypto_LUKS") { "LUKS".into() } else { String::new() },
        inodes_used: if fs.contains("ext4") { Some(0.12) } else { None },
        ..Default::default()
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
        part("nvme0n1p3", "fedora", "crypto_LUKS > btrfs", 997 * gb, Some(870 * gb), "/", "Linux filesystem"),
    ];
    set_detail(&mut b, "d_path", "/dev/nvme0n1");
    set_detail(&mut b, "d_part_table", "GPT");
    let mut disks = vec![a, b];
    // DISKINFO_DEMO_DISKS=8 shows that many sample disks (to check the layout with many drives).
    let want: usize = std::env::var("DISKINFO_DEMO_DISKS").ok().and_then(|v| v.parse().ok()).unwrap_or(2).clamp(2, 32);
    let mut n = 0;
    while disks.len() < want {
        let mut c = disks[n % 2].clone();
        c.dev = format!("sd{}", (b'c' + n as u8) as char);
        c.serial = format!("DEMO{:04}", n + 3);
        disks.push(c);
        n += 1;
    }
    ScanResult { disks, needs_admin: false, message: None, passwordless: false }
}

/// Two months of made-up readings for the sample disks, so the History tab has something to draw.
pub fn demo_history(disks: &[Disk]) -> crate::history::History {
    let mut h = crate::history::History::default();
    let now = crate::history::now();
    let step = 6 * 3600;
    let count = 60 * 4;
    for (k, d) in disks.iter().enumerate() {
        let mut list = Vec::new();
        for i in 0..count {
            let t = now - (count - i) as u64 * step;
            // A slow weekly swing, like a computer used more on some days.
            let wave = ((i as f32) / 28.0 * std::f32::consts::TAU).sin();
            let base = if k == 0 { 36.0 } else { 40.0 };
            list.push(crate::history::Sample {
                t,
                temp: Some((base + 4.0 * wave + (i % 7) as f32 * 0.3) as i32),
                life: d.life.map(|l| (l + 1 - (i * 2 / count) as i32).min(100)),
                written_gb: d.host_writes_gb.map(|w| w.saturating_sub(((count - i) * 12) as u64)),
                reallocated: if k == 0 { Some(if i > count * 2 / 3 { 8 } else if i > count / 3 { 3 } else { 0 }) } else { None },
                pending: if k == 0 { Some(0) } else { None },
                health: if k == 0 && i > count / 3 { 2 } else { 1 },
            });
        }
        h.disks.insert(d.key(), list);
    }
    h
}

/// A few made-up alerts for the sample disks, so the alert history has something to show.
pub fn demo_alerts(disks: &[Disk]) -> crate::alerts::Log {
    let now = crate::history::now();
    let mut log = crate::alerts::Log::default();
    let label = |i: usize| disks.get(i).map(|d| format!("{} ({})", d.model, d.place())).unwrap_or_default();
    log.push(crate::alerts::Event { t: now - 19 * 86_400, kind: "health".into(), disk: label(0), value: "caution".into() });
    log.push(crate::alerts::Event { t: now - 9 * 86_400, kind: "hot".into(), disk: label(1), value: "62".into() });
    log.push(crate::alerts::Event { t: now - 2 * 86_400, kind: "hot".into(), disk: label(0), value: "51".into() });
    log
}

/// Finished sample results for the speed test window, so it can be checked without running a test.
pub fn demo_bench() -> crate::bench::State {
    use crate::bench::{PassResult, Phase, State, PROFILE_NVME};
    let mut s = State::new(&PROFILE_NVME);
    let pass = |mbps: f64, block: f64, workers: f64| {
        let iops = mbps * 1e6 / block;
        Some(PassResult { mbps, iops, latency_us: workers / iops * 1e6 })
    };
    s.results[0].read = pass(3196.7, 1048576.0, 8.0);
    s.results[0].write = pass(2109.65, 1048576.0, 8.0);
    s.results[1].read = pass(3184.78, 131072.0, 32.0);
    s.results[1].write = pass(1365.86, 131072.0, 32.0);
    s.results[2].read = pass(1329.15, 4096.0, 512.0);
    s.results[2].write = pass(194.39, 4096.0, 512.0);
    s.results[3].read = pass(61.08, 4096.0, 1.0);
    s.results[3].write = pass(124.30, 4096.0, 1.0);
    s.phase = Phase::Done;
    s.fraction = 1.0;
    s
}

/// A sample "fake drive" verdict for the window: claims 128 GB, holds about 8 GB.
pub fn demo_fake() -> crate::fakecheck::State {
    use crate::fakecheck::{BadBlock, BlockKind, Config, Phase, State, Verdict};
    let cfg = Config { dir: std::path::PathBuf::new(), bytes: 100 << 30, quick: false };
    let mut s = State::new(&cfg);
    s.phase = Phase::Done;
    s.fraction = 1.0;
    s.written = 102_400;
    s.verified = 102_400;
    s.bad_count = 94_208;
    s.bad = vec![
        BadBlock { index: 0, kind: BlockKind::Overwritten(8192) },
        BadBlock { index: 1, kind: BlockKind::Overwritten(8193) },
        BadBlock { index: 2, kind: BlockKind::Overwritten(8194) },
    ];
    s.hist_write = (0..120).map(|i| 22.0 + 6.0 * ((i as f32) / 9.0).sin() - (i as f32 / 60.0) * 4.0).collect();
    s.hist_read = (0..60).map(|i| 31.0 + 4.0 * ((i as f32) / 5.0).sin()).collect();
    s.write_avg = 18.4;
    s.write_min = 3.1;
    s.read_avg = 31.7;
    s.read_min = 12.9;
    s.verdict = Some(Verdict::Fake { real_bytes: 8_589_934_592 });
    s
}
