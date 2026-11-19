//! Health and temperature rules, following MECHANICS.md.

use crate::disks::{Disk, Status};
use serde::{Deserialize, Serialize};

/// Sentinel the original uses for "no temperature" (0x140013cfc).
pub const TEMP_UNKNOWN: i32 = -1000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Thresholds {
    /// Raw value of 0x05 (reallocated sectors) that triggers Caution.
    pub t05: u16,
    /// Raw value of 0xC5 (pending sectors) that triggers Caution.
    pub tc5: u16,
    /// Raw value of 0xC6 (uncorrectable sectors) that triggers Caution.
    pub tc6: u16,
    /// Remaining life (%) at or below which an SSD is Caution.
    pub tff: u16,
    /// Temperature (°C) at or above which the temperature is shown as alarm.
    pub alarm_temp: i32,
}

impl Thresholds {
    pub fn defaults(is_ssd: bool) -> Self {
        Self {
            t05: 1,
            tc5: 1,
            tc6: 1,
            tff: 10,
            alarm_temp: if is_ssd { 60 } else { 50 },
        }
    }
}

fn is_virtual(model: &str) -> bool {
    ["Parallels", "VMware", "QEMU"].iter().any(|v| model.starts_with(v))
}

/// Sets each attribute's status and the disk's overall health.
pub fn evaluate(d: &mut Disk, th: &Thresholds) {
    for a in &mut d.attrs {
        a.status = Status::Good;
    }
    d.health = if is_virtual(&d.model) || !d.smart_read {
        Status::Unknown
    } else if d.is_nvme {
        evaluate_nvme(d, th)
    } else {
        evaluate_ata(d, th)
    };
}

fn evaluate_nvme(d: &mut Disk, th: &Thresholds) -> Status {
    let mut result = Status::Good;
    if d.nvme_critical.unwrap_or(0) != 0 {
        mark(d, 0x01, Status::Bad);
        return Status::Bad;
    }
    if let (Some(spare), Some(limit)) = (d.nvme_spare, d.nvme_spare_thresh) {
        if (1..=100).contains(&limit) {
            if spare < limit {
                mark(d, 0x03, Status::Bad);
                return Status::Bad;
            }
            if spare == limit && limit != 100 {
                mark(d, 0x03, Status::Caution);
                result = Status::Caution;
            }
        }
    }
    if let Some(life) = d.life {
        if life <= th.tff as i32 {
            mark(d, 0x05, Status::Caution);
            result = Status::Caution;
        }
    }
    result
}

fn evaluate_ata(d: &mut Disk, th: &Thresholds) -> Status {
    if d.attrs.is_empty() {
        return Status::Unknown;
    }
    let mut bad = 0;
    let mut caution = false;
    let is_ssd = d.is_ssd;
    for a in &mut d.attrs {
        if let (Some(cur), Some(t)) = (a.current, a.thresh) {
            if t != 0 && cur < t {
                a.status = Status::Bad;
                bad += 1;
                continue;
            }
        }
        let limit = match a.id {
            0x05 => th.t05,
            0xC5 => th.tc5,
            0xC6 => th.tc6,
            _ => 0,
        };
        let all_ff = a.raw & 0xFFFF_FFFF == 0xFFFF_FFFF;
        if !is_ssd && limit != 0 && !all_ff && (a.raw & 0xFFFF) >= limit as u64 {
            a.status = Status::Caution;
            caution = true;
        }
    }
    if is_ssd {
        if let (Some(life), Some(id)) = (d.life, d.life_attr) {
            if life == 0 {
                mark(d, id, Status::Bad);
                bad += 1;
            } else if life <= th.tff as i32 {
                mark(d, id, Status::Caution);
                caution = true;
            }
        }
    }
    if bad > 0 {
        Status::Bad
    } else if caution {
        Status::Caution
    } else {
        Status::Good
    }
}

fn mark(d: &mut Disk, id: u16, s: Status) {
    if let Some(a) = d.attrs.iter_mut().find(|a| a.id == id) {
        a.status = s;
    }
}

/// Temperature colour: alarm (red) at or above the limit, Unknown without a reading.
pub fn temperature_status(temp: Option<i32>, th: &Thresholds) -> Status {
    match temp {
        None => Status::Unknown,
        Some(t) if t == TEMP_UNKNOWN => Status::Unknown,
        Some(t) if t >= th.alarm_temp => Status::Bad,
        Some(_) => Status::Good,
    }
}
