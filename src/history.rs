//! Per-disk history (temperature, life, data written, bad sectors), kept on this computer so
//! the graphs can show a disk getting worse before it reaches Caution.

use crate::disks::{Disk, Status};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// At most one sample this often per disk.
const MIN_GAP_SECS: u64 = 10 * 60;
/// Oldest samples are dropped beyond this many per disk (~70 days at one per 10 minutes).
const MAX_SAMPLES: usize = 10_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Sample {
    /// Seconds since 1970.
    pub t: u64,
    pub temp: Option<i32>,
    pub life: Option<i32>,
    pub written_gb: Option<u64>,
    pub reallocated: Option<u64>,
    pub pending: Option<u64>,
    /// 0 unknown, 1 good, 2 caution, 3 bad.
    pub health: u8,
}

#[derive(Default, Serialize, Deserialize)]
pub struct History {
    pub disks: HashMap<String, Vec<Sample>>,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn path() -> Option<PathBuf> {
    Some(crate::platform::config_dir()?.join("history.json"))
}

impl History {
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(t) = serde_json::to_string(self) {
            // Write then rename, so a crash never leaves half a file.
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(tmp, p);
            }
        }
    }

    pub fn samples(&self, d: &Disk) -> &[Sample] {
        self.disks.get(&d.key()).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Adds a sample for the disk if the last one is old enough. Returns true when added.
    pub fn record(&mut self, d: &Disk) -> bool {
        if d.serial.is_empty() && d.model.is_empty() {
            return false;
        }
        let raw = |id: u16| d.attrs.iter().find(|a| a.id == id && !d.is_nvme).map(|a| a.raw & 0xFFFF_FFFF);
        let s = Sample {
            t: now(),
            temp: d.temperature,
            life: d.life,
            written_gb: d.host_writes_gb,
            reallocated: raw(0x05),
            pending: raw(0xC5),
            health: match d.health {
                Status::Unknown => 0,
                Status::Good => 1,
                Status::Caution => 2,
                Status::Bad => 3,
            },
        };
        if s.temp.is_none() && s.life.is_none() && s.written_gb.is_none() {
            return false;
        }
        let list = self.disks.entry(d.key()).or_default();
        if list.last().is_some_and(|l| s.t < l.t + MIN_GAP_SECS) {
            return false;
        }
        list.push(s);
        if list.len() > MAX_SAMPLES {
            let extra = list.len() - MAX_SAMPLES;
            list.drain(..extra);
        }
        true
    }
}

/// What an SSD's wear and writing so far suggest about the rest of its life.
#[derive(Clone, Debug, Default)]
pub struct LifeEstimate {
    /// Data written so far, TB.
    pub written_tb: f64,
    /// Total the drive should take at this wear rate, TB.
    pub total_tb: Option<f64>,
    /// Still to write before reaching 0 % life, TB.
    pub remaining_tb: Option<f64>,
    /// Average writing per day over the recorded history, GB.
    pub gb_per_day: Option<f64>,
    /// Days until 0 % life at that pace.
    pub days_left: Option<f64>,
}

/// Estimate from wear %, data written and (when there is enough history) the writing pace.
pub fn estimate(d: &Disk, samples: &[Sample]) -> Option<LifeEstimate> {
    if !d.is_ssd {
        return None;
    }
    let life = d.life?;
    let written_gb = d.host_writes_gb? as f64;
    let mut e = LifeEstimate { written_tb: written_gb / 1000.0, ..Default::default() };
    let used = (100 - life) as f64;
    if used >= 1.0 {
        let total = e.written_tb * 100.0 / used;
        e.total_tb = Some(total);
        e.remaining_tb = Some((total - e.written_tb).max(0.0));
    }
    // Pace: first and last samples at least a week apart that both know the data written.
    let known: Vec<&Sample> = samples.iter().filter(|s| s.written_gb.is_some()).collect();
    if let (Some(first), Some(last)) = (known.first(), known.last()) {
        let days = (last.t.saturating_sub(first.t)) as f64 / 86_400.0;
        let grown = last.written_gb.unwrap_or(0).saturating_sub(first.written_gb.unwrap_or(0)) as f64;
        if days >= 7.0 && grown > 0.0 {
            let per_day = grown / days;
            e.gb_per_day = Some(per_day);
            if let Some(rem) = e.remaining_tb {
                e.days_left = Some(rem * 1000.0 / per_day);
            }
        }
    }
    Some(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_from_wear_and_pace() {
        let d = Disk { is_ssd: true, life: Some(90), host_writes_gb: Some(20_000), ..Default::default() };
        let day = 86_400;
        let samples = vec![
            Sample { t: 0, written_gb: Some(19_000), ..Default::default() },
            Sample { t: 10 * day, written_gb: Some(20_000), ..Default::default() },
        ];
        let e = estimate(&d, &samples).unwrap();
        assert!((e.total_tb.unwrap() - 200.0).abs() < 0.01);
        assert!((e.remaining_tb.unwrap() - 180.0).abs() < 0.01);
        assert!((e.gb_per_day.unwrap() - 100.0).abs() < 0.01);
        assert!((e.days_left.unwrap() - 1800.0).abs() < 0.5);
    }

    #[test]
    fn record_spacing() {
        let mut h = History::default();
        let d = Disk { model: "M".into(), serial: "S".into(), temperature: Some(40), ..Default::default() };
        assert!(h.record(&d));
        assert!(!h.record(&d), "second sample within 10 minutes is skipped");
    }
}
