//! Live read/write speed per disk, sampled in the background while the app is open.
//!
//! Linux: /proc/diskstats (sectors of 512 bytes). Windows: the PhysicalDisk performance
//! counters. macOS: `iostat -Id`, which gives one combined total (no read/write split).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
#[cfg(not(windows))]
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default)]
pub struct Speed {
    /// Bytes per second.
    pub read: Option<f64>,
    pub write: Option<f64>,
    /// Read + write when the system only gives a total (macOS).
    pub total: Option<f64>,
}

/// Shared table: disk key ("sda", "0" for PhysicalDrive0, "disk0") → speed.
pub type Speeds = Arc<Mutex<HashMap<String, Speed>>>;

pub fn start() -> Speeds {
    let speeds: Speeds = Arc::new(Mutex::new(HashMap::new()));
    let out = speeds.clone();
    std::thread::spawn(move || sampler(out));
    speeds
}

/// The key a disk is looked up by.
pub fn key_for(d: &crate::disks::Disk) -> String {
    if cfg!(windows) {
        // PhysicalDriveN → "N"
        d.path_label.trim_start_matches("PhysicalDrive").to_string()
    } else {
        d.dev.clone()
    }
}

#[cfg(target_os = "linux")]
fn sampler(out: Speeds) {
    let read_counts = || -> HashMap<String, (u64, u64)> {
        let text = std::fs::read_to_string("/proc/diskstats").unwrap_or_default();
        text.lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                let name = f.get(2)?.to_string();
                let read: u64 = f.get(5)?.parse().ok()?;
                let written: u64 = f.get(9)?.parse().ok()?;
                Some((name, (read * 512, written * 512)))
            })
            .collect()
    };
    let mut last = read_counts();
    let mut at = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let now = read_counts();
        let secs = at.elapsed().as_secs_f64().max(0.001);
        at = Instant::now();
        let mut map = HashMap::new();
        for (name, (r, w)) in &now {
            if let Some((r0, w0)) = last.get(name) {
                map.insert(
                    name.clone(),
                    Speed {
                        read: Some(r.saturating_sub(*r0) as f64 / secs),
                        write: Some(w.saturating_sub(*w0) as f64 / secs),
                        total: None,
                    },
                );
            }
        }
        last = now;
        if let Ok(mut s) = out.lock() {
            *s = map;
        }
    }
}

#[cfg(windows)]
fn sampler(out: Speeds) {
    let ps = "Get-CimInstance Win32_PerfFormattedData_PerfDisk_PhysicalDisk | Where-Object Name -ne '_Total' | \
              ForEach-Object { \"$($_.Name)|$($_.DiskReadBytesPersec)|$($_.DiskWriteBytesPersec)\" }";
    loop {
        if let Ok(o) = crate::platform::command("powershell").args(["-NoProfile", "-Command", ps]).output() {
            let mut map = HashMap::new();
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let f: Vec<&str> = line.trim().split('|').collect();
                if f.len() != 3 {
                    continue;
                }
                // Names look like "0 C:" or "1": the disk number comes first.
                let Some(num) = f[0].split_whitespace().next() else { continue };
                map.insert(
                    num.to_string(),
                    Speed { read: f[1].parse().ok(), write: f[2].parse().ok(), total: None },
                );
            }
            if let Ok(mut s) = out.lock() {
                *s = map;
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[cfg(target_os = "macos")]
fn sampler(out: Speeds) {
    // `iostat -Id`: first line names the disks, the last line has KB/t, xfrs, MB for each.
    let read_totals = || -> HashMap<String, f64> {
        let Ok(o) = crate::platform::command("iostat").args(["-Id"]).output() else {
            return HashMap::new();
        };
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        let lines: Vec<&str> = text.lines().collect();
        let (Some(names), Some(values)) = (lines.first(), lines.last()) else {
            return HashMap::new();
        };
        let names: Vec<&str> = names.split_whitespace().collect();
        let values: Vec<f64> = values.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        names
            .iter()
            .enumerate()
            .filter_map(|(i, n)| Some((n.to_string(), *values.get(i * 3 + 2)? * 1_048_576.0)))
            .collect()
    };
    let mut last = read_totals();
    let mut at = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let now = read_totals();
        let secs = at.elapsed().as_secs_f64().max(0.001);
        at = Instant::now();
        let mut map = HashMap::new();
        for (name, total) in &now {
            if let Some(t0) = last.get(name) {
                map.insert(name.clone(), Speed { read: None, write: None, total: Some((total - t0).max(0.0) / secs) });
            }
        }
        last = now;
        if let Ok(mut s) = out.lock() {
            *s = map;
        }
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn sampler(_out: Speeds) {}
