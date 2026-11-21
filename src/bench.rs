//! Disk speed tests in the style of CrystalDiskMark: sequential and random reads and writes
//! with a chosen block size, queue depth and thread count, on a temporary test file.
//!
//! How it works, so the numbers can be read with the right expectations:
//! - The system's cache is bypassed (O_DIRECT on Linux, no-buffering on Windows, F_NOCACHE on
//!   macOS). On a file system that cannot do that the test falls back to cached I/O and says so.
//! - A "queue depth" of Q with T threads is emulated with Q × T worker threads that each wait for
//!   one request at a time, so the disk sees about Q × T requests at once. This is not the same
//!   as the asynchronous queues CrystalDiskMark (DiskSpd) uses, so numbers are comparable between
//!   disks tested with DiskInfo++ but may differ somewhat from CrystalDiskMark's.
//! - Data is random (hard for a controller to compress) or a fill of 0x00 / 0xFF.

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Buffers, offsets and sizes are multiples of this (what direct I/O needs).
const ALIGN: usize = 4096;
/// Size of the shared pool the write data is cut from.
const POOL_BYTES: usize = 16 << 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pattern {
    Random,
    Zeros,
    Ones,
}

/// One test: block size, queue depth, threads, sequential or random.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    pub name: &'static str,
    pub block: usize,
    pub sequential: bool,
    pub queue: u32,
    pub threads: u32,
}

impl Spec {
    /// Worker threads that stand for Q × T requests in flight.
    pub fn workers(&self) -> usize {
        (self.queue * self.threads).max(1) as usize
    }
}

const fn spec(name: &'static str, block: usize, sequential: bool, queue: u32, threads: u32) -> Spec {
    Spec { name, block, sequential, queue, threads }
}

/// CrystalDiskMark's "NVMe/SSD" set.
pub const PROFILE_NVME: [Spec; 4] = [
    spec("SEQ1M Q8T1", 1 << 20, true, 8, 1),
    spec("SEQ128K Q32T1", 128 << 10, true, 32, 1),
    spec("RND4K Q32T16", 4096, false, 32, 16),
    spec("RND4K Q1T1", 4096, false, 1, 1),
];

/// CrystalDiskMark's default set.
pub const PROFILE_DEFAULT: [Spec; 4] = [
    spec("SEQ1M Q8T1", 1 << 20, true, 8, 1),
    spec("SEQ1M Q1T1", 1 << 20, true, 1, 1),
    spec("RND4K Q32T1", 4096, false, 32, 1),
    spec("RND4K Q1T1", 4096, false, 1, 1),
];

#[derive(Clone, Debug)]
pub struct Config {
    /// Folder (on the disk to test) where the temporary file is made.
    pub dir: PathBuf,
    pub size: u64,
    /// How many times everything is repeated; the best result of each test is kept.
    pub runs: u32,
    /// Seconds each test is measured for.
    pub secs: f64,
    /// Seconds of rest between tests (lets an SSD settle).
    pub interval: f64,
    pub pattern: Pattern,
    pub specs: Vec<Spec>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PassResult {
    /// Megabytes (10^6 bytes) per second.
    pub mbps: f64,
    pub iops: f64,
    /// Average time of one request, in microseconds (requests in flight ÷ IOPS).
    pub latency_us: f64,
}

#[derive(Clone, Debug)]
pub struct TestResult {
    pub spec: Spec,
    pub read: Option<PassResult>,
    pub write: Option<PassResult>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    /// Making and filling the test file.
    Preparing,
    /// Running test `index` (read, or write when `write`).
    Testing { index: usize, write: bool, run: u32 },
    Done,
    Cancelled,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct State {
    pub phase: Phase,
    /// 0..=1 over the whole run.
    pub fraction: f32,
    pub results: Vec<TestResult>,
    /// False when the file system could not bypass the system cache (results are optimistic).
    pub direct: bool,
}

impl State {
    pub fn new(specs: &[Spec]) -> State {
        State {
            phase: Phase::Idle,
            fraction: 0.0,
            results: specs.iter().map(|s| TestResult { spec: *s, read: None, write: None }).collect(),
            direct: true,
        }
    }

    pub fn running(&self) -> bool {
        matches!(self.phase, Phase::Preparing | Phase::Testing { .. })
    }
}

pub type Shared = Arc<Mutex<State>>;

/// A running (or finished) benchmark.
pub struct Handle {
    pub state: Shared,
    pub cancel: Arc<AtomicBool>,
}

/// Starts the benchmark in a thread and returns at once.
pub fn start(cfg: Config) -> Handle {
    let state: Shared = Arc::new(Mutex::new(State::new(&cfg.specs)));
    let cancel = Arc::new(AtomicBool::new(false));
    let (s, c) = (state.clone(), cancel.clone());
    std::thread::spawn(move || run(cfg, s, c));
    Handle { state, cancel }
}

fn update(state: &Shared, f: impl FnOnce(&mut State)) {
    f(&mut state.lock().unwrap_or_else(|e| e.into_inner()));
}

// ---------------------------------------------------------------------------------- memory

/// Memory aligned for direct I/O.
pub struct AlignedBuf {
    ptr: *mut u8,
    len: usize,
}

// The buffer is plain bytes; sharing it between threads is safe as long as writers are unique.
unsafe impl Send for AlignedBuf {}
unsafe impl Sync for AlignedBuf {}

impl AlignedBuf {
    pub fn new(len: usize) -> AlignedBuf {
        let len = len.max(ALIGN).div_ceil(ALIGN) * ALIGN;
        let layout = Layout::from_size_align(len, ALIGN).expect("valid layout");
        // SAFETY: the layout has a non-zero size.
        let ptr = unsafe { alloc_zeroed(layout) };
        assert!(!ptr.is_null(), "out of memory");
        AlignedBuf { ptr, len }
    }

    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: ptr is valid for len bytes for the life of self.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: as above, and &mut self makes the access unique.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(self.len, ALIGN).expect("valid layout");
        // SAFETY: allocated in `new` with this layout.
        unsafe { dealloc(self.ptr, layout) }
    }
}

/// A small, fast random generator (xorshift64*). Good enough for offsets and test data.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// The data writes are cut from: random bytes, or all 0x00 / 0xFF.
pub fn make_pool(pattern: Pattern) -> AlignedBuf {
    let mut pool = AlignedBuf::new(POOL_BYTES);
    match pattern {
        Pattern::Zeros => {}
        Pattern::Ones => pool.as_mut_slice().fill(0xFF),
        Pattern::Random => {
            let seed = Instant::now().elapsed().as_nanos() as u64
                ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
            let mut rng = Rng::new(seed);
            for chunk in pool.as_mut_slice().chunks_exact_mut(8) {
                chunk.copy_from_slice(&rng.next().to_le_bytes());
            }
        }
    }
    pool
}

// ---------------------------------------------------------------------------------- files

fn open_file(path: &Path, direct: bool) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.read(true).write(true).create_new(true);
    #[cfg(target_os = "linux")]
    if direct {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_DIRECT);
    }
    #[cfg(windows)]
    if direct {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH
        o.custom_flags(0x2000_0000 | 0x8000_0000);
    }
    let f = o.open(path)?;
    #[cfg(target_os = "macos")]
    if direct {
        use std::os::fd::AsRawFd;
        // SAFETY: a valid file descriptor and a plain flag.
        unsafe {
            libc::fcntl(f.as_raw_fd(), libc::F_NOCACHE, 1);
        }
    }
    let _ = direct;
    Ok(f)
}

/// The temporary test file; it is deleted when this is dropped.
pub struct TestFile {
    pub file: Arc<File>,
    pub path: PathBuf,
    /// The system cache is bypassed.
    pub direct: bool,
}

impl TestFile {
    pub fn create(dir: &Path, size: u64) -> io::Result<TestFile> {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let path = dir.join(format!("diskinfo-bench-{}-{stamp}.tmp", std::process::id()));
        let (file, direct) = match open_file(&path, true) {
            Ok(f) => (f, true),
            Err(_) => {
                // Some file systems (tmpfs, certain network shares) refuse direct I/O; a failed
                // open may still have made the file.
                let _ = std::fs::remove_file(&path);
                (open_file(&path, false)?, false)
            }
        };
        let tf = TestFile { file: Arc::new(file), path, direct };
        tf.file.set_len(size)?;
        Ok(tf)
    }
}

impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
fn pread(f: &File, buf: &mut [u8], off: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(f, buf, off)
}
#[cfg(unix)]
fn pwrite(f: &File, buf: &[u8], off: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::write_at(f, buf, off)
}
#[cfg(windows)]
fn pread(f: &File, buf: &mut [u8], off: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(f, buf, off)
}
#[cfg(windows)]
fn pwrite(f: &File, buf: &[u8], off: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_write(f, buf, off)
}

fn read_full(f: &File, buf: &mut [u8], off: u64) -> io::Result<()> {
    let mut done = 0;
    while done < buf.len() {
        let n = pread(f, &mut buf[done..], off + done as u64)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "test file is shorter than expected"));
        }
        done += n;
    }
    Ok(())
}

fn write_full(f: &File, buf: &[u8], off: u64) -> io::Result<()> {
    let mut done = 0;
    while done < buf.len() {
        let n = pwrite(f, &buf[done..], off + done as u64)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "the disk accepted no more data"));
        }
        done += n;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------- passes

/// Sleeps up to `secs`, in small steps, so a cancel is noticed quickly. False if cancelled.
fn wait(secs: f64, cancel: &AtomicBool) -> bool {
    let end = Instant::now() + Duration::from_secs_f64(secs.max(0.0));
    while Instant::now() < end {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25).min(end.saturating_duration_since(Instant::now())));
    }
    !cancel.load(Ordering::Relaxed)
}

/// Fills the whole file once, so reads find real data (and not holes) and writes have space.
fn prefill(file: &Arc<File>, size: u64, pool: &Arc<AlignedBuf>, cancel: &Arc<AtomicBool>, progress: &Arc<AtomicU64>) -> io::Result<()> {
    const BLOCK: usize = 1 << 20;
    let blocks = size / BLOCK as u64;
    let next = Arc::new(AtomicU64::new(0));
    let error: Arc<Mutex<Option<io::Error>>> = Arc::new(Mutex::new(None));
    let mut handles = Vec::new();
    for i in 0..8u64 {
        let (file, pool, cancel, next, error, progress) = (file.clone(), pool.clone(), cancel.clone(), next.clone(), error.clone(), progress.clone());
        handles.push(std::thread::spawn(move || {
            let mut rng = Rng::new(0xF111 + i);
            loop {
                let b = next.fetch_add(1, Ordering::Relaxed);
                if b >= blocks || cancel.load(Ordering::Relaxed) {
                    break;
                }
                let p = (rng.next() as usize % (POOL_BYTES - BLOCK)) & !(ALIGN - 1);
                if let Err(e) = write_full(&file, &pool.as_slice()[p..p + BLOCK], b * BLOCK as u64) {
                    *error.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
                    cancel.store(true, Ordering::Relaxed);
                    break;
                }
                progress.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    let err = error.lock().unwrap_or_else(|e| e.into_inner()).take();
    match err {
        Some(e) => Err(e),
        None => file.sync_data(),
    }
}

/// One measured pass: read or write, for `secs` seconds, with `spec.workers()` requests in flight.
/// `on_progress` gets 0..=1 while it runs. Returns None when cancelled.
fn run_pass(
    file: &Arc<File>,
    spec: Spec,
    write: bool,
    size: u64,
    secs: f64,
    pool: &Arc<AlignedBuf>,
    cancel: &Arc<AtomicBool>,
    on_progress: &dyn Fn(f32),
) -> io::Result<Option<PassResult>> {
    let nblocks = size / spec.block as u64;
    let workers = spec.workers();
    let counters: Vec<Arc<AtomicU64>> = (0..workers).map(|_| Arc::new(AtomicU64::new(0))).collect();
    let end = Arc::new(AtomicBool::new(false));
    let next = Arc::new(AtomicU64::new(0));
    let ready = Arc::new(AtomicUsize::new(0));
    let error: Arc<Mutex<Option<io::Error>>> = Arc::new(Mutex::new(None));
    let mut handles = Vec::new();
    for (i, counter) in counters.iter().enumerate() {
        let (file, pool, cancel, end, next, ready, error, counter) =
            (file.clone(), pool.clone(), cancel.clone(), end.clone(), next.clone(), ready.clone(), error.clone(), counter.clone());
        let block = spec.block;
        let sequential = spec.sequential;
        let h = std::thread::Builder::new().stack_size(256 << 10).spawn(move || {
            let mut rng = Rng::new(0xB3C4 + i as u64 * 7919);
            let mut buf = AlignedBuf::new(block);
            ready.fetch_add(1, Ordering::Release);
            while !end.load(Ordering::Relaxed) && !cancel.load(Ordering::Relaxed) {
                let blk = if sequential { next.fetch_add(1, Ordering::Relaxed) % nblocks } else { rng.next() % nblocks };
                let off = blk * block as u64;
                let r = if write {
                    let p = (rng.next() as usize % (POOL_BYTES - block)) & !(ALIGN - 1);
                    write_full(&file, &pool.as_slice()[p..p + block], off)
                } else {
                    read_full(&file, &mut buf.as_mut_slice()[..block], off)
                };
                if let Err(e) = r {
                    *error.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
                    cancel.store(true, Ordering::Relaxed);
                    break;
                }
                counter.fetch_add(1, Ordering::Relaxed);
            }
        })?;
        handles.push(h);
    }
    // Start the clock when every worker is up (spawning hundreds of threads takes a moment).
    let spawn_deadline = Instant::now() + Duration::from_secs(5);
    while ready.load(Ordering::Acquire) < workers && Instant::now() < spawn_deadline && !cancel.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(2));
    }
    let total = |c: &[Arc<AtomicU64>]| c.iter().map(|c| c.load(Ordering::Relaxed)).sum::<u64>();
    // The first fifth of the time (at most a second) is warm-up and is not counted.
    let warm = (secs * 0.2).min(1.0);
    let started = Instant::now();
    let mut alive = wait(warm, cancel);
    let (t_a, ops_a) = (Instant::now(), total(&counters));
    if alive {
        let measure = (secs - warm).max(0.05);
        let t0 = Instant::now();
        while alive && t0.elapsed().as_secs_f64() < measure {
            alive = wait(0.05, cancel);
            on_progress((started.elapsed().as_secs_f64() / secs).min(1.0) as f32);
        }
    }
    let (t_b, ops_b) = (Instant::now(), total(&counters));
    end.store(true, Ordering::Relaxed);
    for h in handles {
        let _ = h.join();
    }
    let err = error.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(e) = err {
        return Err(e);
    }
    if !alive || cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    if write {
        file.sync_data()?;
    }
    let dt = t_b.duration_since(t_a).as_secs_f64().max(1e-6);
    let iops = (ops_b - ops_a) as f64 / dt;
    Ok(Some(PassResult {
        mbps: iops * spec.block as f64 / 1e6,
        iops,
        latency_us: if iops > 0.0 { workers as f64 / iops * 1e6 } else { 0.0 },
    }))
}

/// Keeps the better of two results.
fn best(old: Option<PassResult>, new: PassResult) -> PassResult {
    match old {
        Some(o) if o.mbps >= new.mbps => o,
        _ => new,
    }
}

// ---------------------------------------------------------------------------------- the run

/// Runs the whole benchmark (blocking). Progress and results go to `state`.
pub fn run(cfg: Config, state: Shared, cancel: Arc<AtomicBool>) {
    let result = run_inner(&cfg, &state, &cancel);
    update(&state, |s| match result {
        Ok(true) => {
            s.phase = Phase::Done;
            s.fraction = 1.0;
        }
        Ok(false) => s.phase = Phase::Cancelled,
        Err(e) => s.phase = Phase::Failed(e.to_string()),
    });
}

fn run_inner(cfg: &Config, state: &Shared, cancel: &Arc<AtomicBool>) -> io::Result<bool> {
    // Whole megabytes, and room for the biggest block.
    let size = (cfg.size / (1 << 20)).max(1) * (1 << 20);
    update(state, |s| s.phase = Phase::Preparing);
    let tf = TestFile::create(&cfg.dir, size)?;
    update(state, |s| s.direct = tf.direct);
    let pool = Arc::new(make_pool(cfg.pattern));

    let passes = cfg.specs.len() as u32 * 2 * cfg.runs.max(1);
    // Preparing counts as one tenth of the run, the passes share the rest.
    let progress = Arc::new(AtomicU64::new(0));
    let blocks = size / (1 << 20);
    let watcher = {
        let (progress, state, cancel) = (progress.clone(), state.clone(), cancel.clone());
        std::thread::spawn(move || {
            while progress.load(Ordering::Relaxed) < blocks && !cancel.load(Ordering::Relaxed) {
                let f = progress.load(Ordering::Relaxed) as f32 / blocks as f32 * 0.1;
                update(&state, |s| s.fraction = f);
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };
    let pre = prefill(&tf.file, size, &pool, cancel, &progress);
    progress.store(blocks, Ordering::Relaxed);
    let _ = watcher.join();
    pre?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(false);
    }

    let mut done = 0u32;
    for run in 0..cfg.runs.max(1) {
        // Like CrystalDiskMark: every read test first, then every write test.
        for write in [false, true] {
            for (index, spec) in cfg.specs.iter().enumerate() {
                update(state, |s| s.phase = Phase::Testing { index, write, run });
                let base = 0.1 + 0.9 * done as f32 / passes as f32;
                let step = 0.9 / passes as f32;
                let st = state.clone();
                let on_progress = move |f: f32| update(&st, |s| s.fraction = base + step * f);
                let r = run_pass(&tf.file, *spec, write, size, cfg.secs, &pool, cancel, &on_progress)?;
                let Some(r) = r else { return Ok(false) };
                update(state, |s| {
                    let t = &mut s.results[index];
                    if write {
                        t.write = Some(best(t.write, r));
                    } else {
                        t.read = Some(best(t.read, r));
                    }
                });
                done += 1;
                if done < passes && !wait(cfg.interval, cancel) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// A plain-text table of the results (labels come from the caller, already translated).
pub fn format_table(results: &[TestResult], read: &str, write: &str) -> String {
    let mut out = format!("{:<16} {:>12} {:>10} {:>10}   {:>12} {:>10} {:>10}\n", "", format!("{read} MB/s"), "IOPS", "µs", format!("{write} MB/s"), "IOPS", "µs");
    let cell = |r: &Option<PassResult>| match r {
        Some(p) => format!("{:>12.2} {:>10.0} {:>10.1}", p.mbps, p.iops, p.latency_us),
        None => format!("{:>12} {:>10} {:>10}", "—", "—", "—"),
    };
    for t in results {
        out += &format!("{:<16} {}   {}\n", t.spec.name, cell(&t.read), cell(&t.write));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pools_follow_the_chosen_pattern() {
        assert!(make_pool(Pattern::Zeros).as_slice().iter().all(|b| *b == 0));
        assert!(make_pool(Pattern::Ones).as_slice().iter().all(|b| *b == 0xFF));
        let r = make_pool(Pattern::Random);
        let distinct = r.as_slice()[..4096].iter().collect::<std::collections::HashSet<_>>().len();
        assert!(distinct > 200, "random data has many different bytes: {distinct}");
        assert_eq!(r.as_slice().as_ptr() as usize % ALIGN, 0, "aligned for direct I/O");
    }

    #[test]
    fn profiles_are_the_crystaldiskmark_sets() {
        let names: Vec<&str> = PROFILE_NVME.iter().map(|s| s.name).collect();
        assert_eq!(names, ["SEQ1M Q8T1", "SEQ128K Q32T1", "RND4K Q32T16", "RND4K Q1T1"]);
        assert_eq!(PROFILE_NVME[2].workers(), 512);
        assert_eq!(PROFILE_NVME[3].workers(), 1);
        assert_eq!(PROFILE_DEFAULT[1].name, "SEQ1M Q1T1");
    }

    #[test]
    fn a_small_run_measures_every_test_and_cleans_up() {
        let dir = std::env::temp_dir().join(format!("diskinfo-bench-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config {
            dir: dir.clone(),
            size: 16 << 20,
            runs: 1,
            secs: 0.3,
            interval: 0.0,
            pattern: Pattern::Random,
            specs: PROFILE_NVME.to_vec(),
        };
        let state = Arc::new(Mutex::new(State::new(&cfg.specs)));
        run(cfg, state.clone(), Arc::new(AtomicBool::new(false)));
        let s = state.lock().unwrap();
        assert_eq!(s.phase, Phase::Done, "{:?}", s.phase);
        for t in &s.results {
            let (r, w) = (t.read.expect("read measured"), t.write.expect("write measured"));
            assert!(r.mbps > 0.0 && w.mbps > 0.0 && r.iops > 0.0, "{t:?}");
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "the test file was deleted");
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn cancelling_stops_the_run() {
        let dir = std::env::temp_dir().join(format!("diskinfo-bench-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config { dir: dir.clone(), size: 16 << 20, runs: 3, secs: 5.0, interval: 0.0, pattern: Pattern::Zeros, specs: PROFILE_DEFAULT.to_vec() };
        let h = start(cfg);
        std::thread::sleep(Duration::from_millis(400));
        h.cancel.store(true, Ordering::Relaxed);
        for _ in 0..100 {
            if !h.state.lock().unwrap().running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(h.state.lock().unwrap().phase, Phase::Cancelled);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "the test file was deleted");
        let _ = std::fs::remove_dir(&dir);
    }
}
