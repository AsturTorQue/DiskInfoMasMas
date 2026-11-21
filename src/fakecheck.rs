//! "Is this drive real?" — a test for fake USB sticks, memory cards and SSDs whose real size is
//! smaller than the one they report (like H2testw or F3 do).
//!
//! It writes a pattern that is different in every 1 MiB block (the block number is inside it),
//! then reads everything back and compares. A fake drive gives itself away because:
//! - data written beyond its real size wraps around and overwrites older blocks ("overwritten"),
//! - or it is silently thrown away and reads back empty or as garbage.
//!
//! The data goes into files of 1 GiB (so it also works on FAT32) in a folder of the drive to test.
//! The more of the free space is tested, the more sure the verdict: a fake drive only shows itself
//! once more than its real capacity has been written.

use crate::bench::{self, AlignedBuf, Rng};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use serde::{Deserialize, Serialize};

pub const BLOCK: usize = 1 << 20;
/// Blocks in each test file (1 GiB, under the 4 GiB limit of FAT32).
const BLOCKS_PER_FILE: u64 = 1024;
const MAGIC: &[u8; 8] = b"DIFAKE01";
const HEADER: usize = 24;
/// Only this many wrong blocks are listed (all are counted).
const MAX_LISTED: usize = 40;

#[derive(Clone, Debug)]
pub struct Config {
    /// Folder (on the drive to test) where the test files are made.
    pub dir: PathBuf,
    /// How much to write and check; rounded down to whole MiB. In quick mode: how much space
    /// the samples are spread over.
    pub bytes: u64,
    /// Quick mode (like ValiDrive): a few hundred blocks spread evenly over the whole space
    /// instead of every block.
    pub quick: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Serialize, Deserialize)]
pub enum BlockKind {
    /// It holds the data of another block (the drive wrapped around): the other block's number.
    Overwritten(u64),
    /// It reads back as zeros or cannot be read.
    Empty,
    /// It holds something else.
    Corrupt,
}

#[derive(Clone, Copy, Debug)]
#[derive(Serialize, Deserialize)]
pub struct BadBlock {
    pub index: u64,
    pub kind: BlockKind,
}

#[derive(Clone, Debug, PartialEq)]
#[derive(Serialize, Deserialize)]
pub enum Phase {
    Idle,
    Writing,
    Verifying,
    Done,
    Cancelled,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
#[derive(Serialize, Deserialize)]
pub enum Verdict {
    /// Everything came back as written.
    Genuine,
    /// The drive holds much less than it claims; about this many bytes are real.
    Fake { real_bytes: u64 },
    /// A few blocks are wrong: a failing drive, not a fake size.
    Faulty { bad_blocks: u64 },
    /// No errors, but the system cache could not be bypassed, so reading back proves little.
    Unreliable,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct State {
    pub phase: Phase,
    /// 0..=1 over writing and verifying.
    pub fraction: f32,
    /// Blocks asked for, written so far and checked so far.
    pub total: u64,
    pub written: u64,
    pub verified: u64,
    /// Average and slowest speed (MB/s, over windows of about a second) of each phase.
    pub write_avg: f64,
    pub write_min: f64,
    pub read_avg: f64,
    pub read_min: f64,
    pub bad: Vec<BadBlock>,
    pub bad_count: u64,
    /// The drive filled up before the amount asked for was written.
    pub ran_full: bool,
    /// The system cache is bypassed (otherwise reading back may come from memory).
    pub direct: bool,
    pub verdict: Option<Verdict>,
    /// Quick mode: `total`, `written` and `verified` count sample blocks, not the whole space.
    pub quick: bool,
}

impl State {
    pub fn new(cfg: &Config) -> State {
        State {
            phase: Phase::Idle,
            fraction: 0.0,
            total: plan(cfg).len() as u64,
            written: 0,
            verified: 0,
            write_avg: 0.0,
            write_min: 0.0,
            read_avg: 0.0,
            read_min: 0.0,
            bad: Vec::new(),
            bad_count: 0,
            ran_full: false,
            direct: true,
            verdict: None,
            quick: cfg.quick,
        }
    }

    pub fn running(&self) -> bool {
        matches!(self.phase, Phase::Idle | Phase::Writing | Phase::Verifying)
    }
}

pub type Shared = Arc<Mutex<State>>;

pub struct Handle {
    pub state: Shared,
    pub cancel: Arc<AtomicBool>,
}

pub fn start(cfg: Config) -> Handle {
    let state: Shared = Arc::new(Mutex::new(State::new(&cfg)));
    let cancel = Arc::new(AtomicBool::new(false));
    let (s, c) = (state.clone(), cancel.clone());
    std::thread::spawn(move || run(cfg, s, c));
    Handle { state, cancel }
}

fn update(state: &Shared, f: impl FnOnce(&mut State)) {
    f(&mut state.lock().unwrap_or_else(|e| e.into_inner()));
}

/// Samples in quick mode (more on very big drives, up to one per GiB).
const QUICK_SAMPLES: u64 = 256;

/// The block numbers to write and check, in order. Everything for the full test; for the quick
/// one, blocks evenly spread (the step is a power of two, so the spots a wrapping fake drive
/// folds onto one another are also spots that are tested) plus the very last block.
pub fn plan(cfg: &Config) -> Vec<u64> {
    let total = cfg.bytes / BLOCK as u64;
    if !cfg.quick {
        return (0..total).collect();
    }
    let mut stride = 1u64;
    while stride * 2 <= (total / QUICK_SAMPLES).max(1) && stride * 2 <= BLOCKS_PER_FILE {
        stride *= 2;
    }
    let mut v: Vec<u64> = (0..total).step_by(stride as usize).collect();
    if total > 0 && v.last() != Some(&(total - 1)) {
        v.push(total - 1);
    }
    v
}

/// Reserves the space of a file so its blocks sit all over the drive (quick mode).
fn preallocate(file: &std::fs::File, bytes: u64) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: a valid descriptor and plain numbers.
        let r = unsafe { libc::posix_fallocate(file.as_raw_fd(), 0, bytes as libc::off_t) };
        match r {
            0 => return Ok(()),
            28 => return Err(io::Error::from_raw_os_error(28)),
            _ => {}
        }
    }
    file.set_len(bytes)
}

// ------------------------------------------------------------------------- the pattern

/// Fills `buf` (one block) with data that is unique to (seed, index): a header with both, then
/// pseudo-random bytes that depend on both.
pub fn fill_block(buf: &mut [u8], seed: u64, index: u64) {
    buf[..8].copy_from_slice(MAGIC);
    buf[8..16].copy_from_slice(&seed.to_le_bytes());
    buf[16..24].copy_from_slice(&index.to_le_bytes());
    let mut rng = Rng::new(seed ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1FA_CE00);
    for chunk in buf[HEADER..].chunks_exact_mut(8) {
        chunk.copy_from_slice(&rng.next().to_le_bytes());
    }
}

/// Says what a block that is not as expected holds.
pub fn classify(found: &[u8], seed: u64, index: u64) -> BlockKind {
    if found[..8] == MAGIC[..] && found[8..16] == seed.to_le_bytes() {
        let other = u64::from_le_bytes(found[16..24].try_into().unwrap_or([0; 8]));
        if other != index {
            return BlockKind::Overwritten(other);
        }
        return BlockKind::Corrupt;
    }
    if found[..4096.min(found.len())].iter().all(|b| *b == 0) {
        BlockKind::Empty
    } else {
        BlockKind::Corrupt
    }
}

// ------------------------------------------------------------------------- files

/// The test files; they are deleted when this is dropped.
pub struct Files {
    dir: PathBuf,
    tag: String,
    count: u64,
}

impl Files {
    pub fn new(dir: &Path) -> Files {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        Files { dir: dir.to_path_buf(), tag: format!("diskinfo-fake-{}-{stamp}", std::process::id()), count: 0 }
    }

    fn path(&self, n: u64) -> PathBuf {
        self.dir.join(format!("{}-{n:05}.tmp", self.tag))
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        for n in 0..self.count {
            let _ = std::fs::remove_file(self.path(n));
        }
    }
}

/// Measures speed over windows of about a second, keeping the slowest.
struct Meter {
    start: Instant,
    window: Instant,
    window_bytes: u64,
    total_bytes: u64,
    min: Option<f64>,
}

impl Meter {
    fn new() -> Meter {
        Meter { start: Instant::now(), window: Instant::now(), window_bytes: 0, total_bytes: 0, min: None }
    }

    fn add(&mut self, bytes: u64) {
        self.window_bytes += bytes;
        self.total_bytes += bytes;
        let dt = self.window.elapsed().as_secs_f64();
        if dt >= 1.0 {
            let mbps = self.window_bytes as f64 / dt / 1e6;
            self.min = Some(self.min.map_or(mbps, |m| m.min(mbps)));
            self.window = Instant::now();
            self.window_bytes = 0;
        }
    }

    /// (average, slowest) in MB/s.
    fn result(&self) -> (f64, f64) {
        let avg = self.total_bytes as f64 / self.start.elapsed().as_secs_f64().max(1e-6) / 1e6;
        (avg, self.min.unwrap_or(avg))
    }
}

fn disk_full(e: &io::Error) -> bool {
    // ENOSPC (28) on Unix, ERROR_DISK_FULL (112) and ERROR_HANDLE_DISK_FULL (39) on Windows.
    e.kind() == io::ErrorKind::StorageFull || matches!(e.raw_os_error(), Some(28) | Some(112) | Some(39))
}

// ------------------------------------------------------------------------- the two phases

/// Blocks a test file holds (the last one may be shorter).
fn blocks_in_file(cfg: &Config, n: u64) -> u64 {
    (cfg.bytes / BLOCK as u64).saturating_sub(n * BLOCKS_PER_FILE).min(BLOCKS_PER_FILE)
}

/// Writes the blocks of `plan`. Returns how many were written (None when cancelled).
pub fn write_phase(cfg: &Config, plan: &[u64], state: &Shared, cancel: &AtomicBool, files: &mut Files, seed: u64) -> io::Result<Option<u64>> {
    let total = plan.len() as u64;
    let mut buf = AlignedBuf::new(BLOCK);
    let mut meter = Meter::new();
    let mut current: Option<(u64, std::fs::File)> = None;
    let mut written = 0u64;
    let mut ran_full = false;
    for (k, &i) in plan.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let fno = i / BLOCKS_PER_FILE;
        if current.as_ref().map(|c| c.0) != Some(fno) {
            if let Some((_, done)) = current.take() {
                done.sync_data()?;
            }
            let (f, direct) = bench::create_direct_or_cached(&files.path(fno))?;
            files.count = files.count.max(fno + 1);
            if k == 0 {
                update(state, |s| s.direct = direct);
            }
            if cfg.quick {
                match preallocate(&f, blocks_in_file(cfg, fno) * BLOCK as u64) {
                    Ok(()) => {}
                    Err(e) if disk_full(&e) => {
                        ran_full = true;
                        break;
                    }
                    Err(e) => return Err(e),
                }
            }
            current = Some((fno, f));
        }
        fill_block(buf.as_mut_slice(), seed, i);
        let file = &current.as_ref().expect("a file is open").1;
        match bench::write_full(file, &buf.as_slice()[..BLOCK], (i % BLOCKS_PER_FILE) * BLOCK as u64) {
            Ok(()) => {}
            Err(e) if disk_full(&e) => {
                ran_full = true;
                break;
            }
            Err(e) => return Err(e),
        }
        written = k as u64 + 1;
        meter.add(BLOCK as u64);
        if written % 8 == 0 || written == total {
            let f = 0.5 * written as f32 / total.max(1) as f32;
            update(state, |s| {
                s.written = written;
                s.fraction = f;
            });
        }
    }
    if let Some((_, done)) = current.take() {
        done.sync_data()?;
    }
    let (avg, min) = meter.result();
    update(state, |s| {
        s.written = written;
        s.write_avg = avg;
        s.write_min = min;
        s.ran_full = ran_full;
        s.total = if ran_full { written } else { s.total };
    });
    Ok(Some(written))
}

/// Reads the first `written` blocks of `plan` back and counts the ones that are not as written.
/// Returns None if cancelled.
pub fn verify_phase(state: &Shared, plan: &[u64], cancel: &AtomicBool, files: &Files, written: u64, seed: u64) -> io::Result<Option<()>> {
    let direct = state.lock().unwrap_or_else(|e| e.into_inner()).direct;
    let mut found = AlignedBuf::new(BLOCK);
    let mut expected = AlignedBuf::new(BLOCK);
    let mut meter = Meter::new();
    let mut current: Option<(u64, std::fs::File)> = None;
    for (k, &i) in plan.iter().take(written as usize).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let fno = i / BLOCKS_PER_FILE;
        if current.as_ref().map(|c| c.0) != Some(fno) {
            let f = bench::open_existing(&files.path(fno), direct)?;
            #[cfg(target_os = "linux")]
            if !direct {
                // Without direct I/O, ask the system to forget what it still remembers of the file.
                use std::os::fd::AsRawFd;
                // SAFETY: a valid descriptor; the call only gives advice to the kernel.
                unsafe {
                    libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
                }
            }
            current = Some((fno, f));
        }
        let file = &current.as_ref().expect("a file is open").1;
        let read = bench::read_full(file, &mut found.as_mut_slice()[..BLOCK], (i % BLOCKS_PER_FILE) * BLOCK as u64);
        fill_block(expected.as_mut_slice(), seed, i);
        let kind = match read {
            Ok(()) if found.as_slice()[..BLOCK] == expected.as_slice()[..BLOCK] => None,
            Ok(()) => Some(classify(&found.as_slice()[..BLOCK], seed, i)),
            // A block that cannot be read counts as lost.
            Err(_) => Some(BlockKind::Empty),
        };
        meter.add(BLOCK as u64);
        let verified = k as u64 + 1;
        let f = 0.5 + 0.5 * verified as f32 / written.max(1) as f32;
        update(state, |s| {
            s.verified = verified;
            s.fraction = f;
            if let Some(kind) = kind {
                s.bad_count += 1;
                if s.bad.len() < MAX_LISTED {
                    s.bad.push(BadBlock { index: i, kind });
                }
            }
        });
    }
    let (avg, min) = meter.result();
    update(state, |s| {
        s.read_avg = avg;
        s.read_min = min;
    });
    Ok(Some(()))
}

/// The verdict from what was found.
pub fn judge(s: &State) -> Verdict {
    if s.bad_count == 0 {
        return if s.direct { Verdict::Genuine } else { Verdict::Unreliable };
    }
    let overwritten = s.bad.iter().any(|b| matches!(b.kind, BlockKind::Overwritten(_)));
    // Blocks that came back right are what the drive really holds. In quick mode the samples
    // are in order, so the first block that failed (or the first one that overwrote an older
    // one) is about where the real data ends.
    let good = if s.quick {
        // An overwritten block i that now holds block o shows the drive wraps every o - i blocks.
        s.bad.iter().map(|b| match b.kind {
            BlockKind::Overwritten(o) => o.saturating_sub(b.index),
            _ => b.index,
        }).min().unwrap_or(0)
    } else {
        s.written.saturating_sub(s.bad_count)
    };
    if overwritten || s.bad_count * 10 >= s.written.max(1) {
        Verdict::Fake { real_bytes: good * BLOCK as u64 }
    } else {
        Verdict::Faulty { bad_blocks: s.bad_count }
    }
}

/// Runs the whole test (blocking). Progress and the verdict go to `state`.
pub fn run(cfg: Config, state: Shared, cancel: Arc<AtomicBool>) {
    let result = run_inner(&cfg, &state, &cancel);
    update(&state, |s| match result {
        Ok(true) => {
            s.phase = Phase::Done;
            s.fraction = 1.0;
            s.verdict = Some(judge(s));
        }
        Ok(false) => s.phase = Phase::Cancelled,
        Err(e) => s.phase = Phase::Failed(e.to_string()),
    });
}

fn run_inner(cfg: &Config, state: &Shared, cancel: &AtomicBool) -> io::Result<bool> {
    if cfg.bytes < 4 * BLOCK as u64 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the amount to test is too small"));
    }
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
    let plan = plan(cfg);
    let mut files = Files::new(&cfg.dir);
    update(state, |s| s.phase = Phase::Writing);
    let Some(written) = write_phase(cfg, &plan, state, cancel, &mut files, seed)? else { return Ok(false) };
    if written == 0 {
        return Err(io::Error::new(io::ErrorKind::StorageFull, "nothing could be written"));
    }
    update(state, |s| s.phase = Phase::Verifying);
    Ok(verify_phase(state, &plan, cancel, &files, written, seed)?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom, Write};

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("diskinfo-fake-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn setup(name: &str, mib: u64) -> (Config, Shared, AtomicBool, Files, PathBuf) {
        setup_mode(name, mib, false)
    }

    fn setup_mode(name: &str, mib: u64, quick: bool) -> (Config, Shared, AtomicBool, Files, PathBuf) {
        let dir = temp_dir(name);
        let cfg = Config { dir: dir.clone(), bytes: mib << 20, quick };
        let state = Arc::new(Mutex::new(State::new(&cfg)));
        let files = Files::new(&dir);
        (cfg, state, AtomicBool::new(false), files, dir)
    }

    /// Writes, lets the test tamper with the first file (like a fake drive would), verifies.
    fn run_with(name: &str, mib: u64, tamper: impl FnOnce(&Path)) -> State {
        run_mode(name, mib, false, tamper)
    }

    fn run_mode(name: &str, mib: u64, quick: bool, tamper: impl FnOnce(&Path)) -> State {
        let (cfg, state, cancel, mut files, dir) = setup_mode(name, mib, quick);
        let plan = plan(&cfg);
        // The temp folder may be tmpfs (no direct I/O); the verdict logic is what is tested here.
        let written = write_phase(&cfg, &plan, &state, &cancel, &mut files, 77).unwrap().unwrap();
        assert_eq!(written, plan.len() as u64);
        tamper(&files.path(0));
        verify_phase(&state, &plan, &cancel, &files, written, 77).unwrap().unwrap();
        drop(files);
        let _ = std::fs::remove_dir(&dir);
        let mut s = state.lock().unwrap().clone();
        s.direct = true; // judge as if the cache had been bypassed
        s
    }

    fn block_bytes(path: &Path, i: u64) -> Vec<u8> {
        let mut f = std::fs::File::open(path).unwrap();
        f.seek(SeekFrom::Start(i * BLOCK as u64)).unwrap();
        let mut v = vec![0u8; BLOCK];
        f.read_exact(&mut v).unwrap();
        v
    }

    fn put_block(path: &Path, i: u64, data: &[u8]) {
        let mut f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.seek(SeekFrom::Start(i * BLOCK as u64)).unwrap();
        f.write_all(data).unwrap();
        f.sync_all().unwrap();
    }

    #[test]
    fn every_block_is_unique_and_recognisable() {
        let (mut a, mut b) = (vec![0u8; BLOCK], vec![0u8; BLOCK]);
        fill_block(&mut a, 1, 5);
        fill_block(&mut b, 1, 6);
        assert_ne!(a, b);
        let mut again = vec![0u8; BLOCK];
        fill_block(&mut again, 1, 5);
        assert_eq!(a, again, "same block, same data");
        assert_eq!(classify(&b, 1, 5), BlockKind::Overwritten(6));
        assert_eq!(classify(&vec![0u8; BLOCK], 1, 5), BlockKind::Empty);
        assert_eq!(classify(&vec![0x5Au8; BLOCK], 1, 5), BlockKind::Corrupt);
    }

    #[test]
    fn a_good_drive_passes() {
        let s = run_with("good", 8, |_| {});
        assert_eq!((s.bad_count, s.written, s.verified), (0, 8, 8));
        assert_eq!(judge(&s), Verdict::Genuine);
    }

    #[test]
    fn data_that_wraps_around_is_a_fake() {
        // Block 5 landed on top of block 0, as on a drive that only has 5 MiB.
        let s = run_with("wrap", 8, |p| {
            let five = block_bytes(p, 5);
            put_block(p, 0, &five);
        });
        assert_eq!(s.bad_count, 1);
        assert_eq!(s.bad[0].kind, BlockKind::Overwritten(5));
        assert_eq!(judge(&s), Verdict::Fake { real_bytes: 7 << 20 });
    }

    #[test]
    fn writes_thrown_away_are_a_fake() {
        // Everything after the first 4 MiB reads back as zeros.
        let s = run_with("zeros", 8, |p| {
            for i in 4..8 {
                put_block(p, i, &vec![0u8; BLOCK]);
            }
        });
        assert_eq!(s.bad_count, 4);
        assert!(s.bad.iter().all(|b| b.kind == BlockKind::Empty));
        assert_eq!(judge(&s), Verdict::Fake { real_bytes: 4 << 20 });
    }

    #[test]
    fn one_bad_block_is_a_failing_drive_not_a_fake() {
        let s = run_with("faulty", 40, |p| put_block(p, 20, &vec![0xAAu8; BLOCK]));
        assert_eq!(s.bad_count, 1);
        assert_eq!(judge(&s), Verdict::Faulty { bad_blocks: 1 });
    }

    #[test]
    fn no_errors_without_direct_io_proves_little() {
        let mut s = run_with("cached", 8, |_| {});
        s.direct = false;
        assert_eq!(judge(&s), Verdict::Unreliable);
    }

    #[test]
    fn cancelling_removes_the_files() {
        let (cfg, state, cancel, files, dir) = setup("cancel", 8);
        cancel.store(true, Ordering::Relaxed);
        let mut files = files;
        let plan = plan(&cfg);
        assert!(write_phase(&cfg, &plan, &state, &cancel, &mut files, 3).unwrap().is_none());
        drop(files);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn the_quick_plan_spreads_over_the_whole_space() {
        let cfg = Config { dir: PathBuf::new(), bytes: 512 << 20, quick: true };
        let p = plan(&cfg);
        assert_eq!(p[0], 0);
        assert_eq!(*p.last().unwrap(), 511);
        assert!(p.len() >= 256 && p.len() <= 260, "{}", p.len());
        assert!(p.windows(2).all(|w| w[0] < w[1]));
        let big = Config { dir: PathBuf::new(), bytes: 2000u64 << 30, quick: true };
        assert!(plan(&big).len() < 4000);
        assert_eq!(plan(&Config { quick: false, bytes: 5 << 20, ..cfg }), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn quick_mode_passes_a_good_drive() {
        let s = run_mode("qgood", 512, true, |_| {});
        assert_eq!(s.bad_count, 0);
        assert_eq!(judge(&s), Verdict::Genuine);
    }

    #[test]
    fn quick_mode_catches_a_wrapping_drive() {
        // A drive of 256 MiB claiming 512: block i + 256 lands on top of block i.
        let s = run_mode("qwrap", 512, true, |p| {
            for i in (0..256).step_by(2) {
                let later = block_bytes(p, i + 256);
                put_block(p, i, &later);
            }
        });
        assert!(s.bad_count > 100);
        match judge(&s) {
            Verdict::Fake { real_bytes } => assert_eq!(real_bytes, 256 << 20),
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn quick_mode_catches_discarded_writes() {
        // Everything past 100 MiB reads back as zeros.
        let s = run_mode("qzero", 512, true, |p| {
            for i in (100..512).step_by(2) {
                put_block(p, i, &vec![0u8; BLOCK]);
            }
        });
        match judge(&s) {
            Verdict::Fake { real_bytes } => assert!((98 << 20..=102 << 20).contains(&real_bytes), "{real_bytes}"),
            v => panic!("{v:?}"),
        }
    }
}
