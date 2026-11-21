//! Quick fake-drive test on the raw drive (Linux), the way ValiDrive does it: a few hundred
//! small spots spread evenly over the WHOLE drive are saved, overwritten with unique data,
//! read back and then put back as they were. No file system is involved, so it is as fast on
//! a 128 GB stick as on a small one, and it reaches the end of the drive.
//!
//! Opening a drive needs administrator rights, so the window starts a copy of this program as
//! root (`diskinfo rawcheck /dev/sdX`, through pkexec) and reads its progress lines. The drive
//! must not be mounted: the spots are put back, but a file system writing at the same moment
//! could be damaged.

use crate::fakecheck::{self, BadBlock, BlockKind, Config, Handle, Phase, Shared, State, BLOCK};
use serde_json::Value;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Bytes tested at each spot.
const CHUNK: usize = 64 << 10;
const PREFIX: &str = "STATE ";

fn update(state: &Shared, f: impl FnOnce(&mut State)) {
    f(&mut state.lock().unwrap_or_else(|e| e.into_inner()));
}

/// Whether any partition of the drive (or the drive) is mounted.
fn mounted(dev: &str) -> bool {
    std::fs::read_to_string("/proc/mounts")
        .map(|t| t.lines().any(|l| l.split_whitespace().next().is_some_and(|d| d.starts_with(dev))))
        .unwrap_or(false)
}

fn size_of(f: &std::fs::File) -> io::Result<u64> {
    use std::os::fd::AsRawFd;
    // SAFETY: a valid descriptor; lseek only moves its offset.
    let n = unsafe { libc::lseek(f.as_raw_fd(), 0, libc::SEEK_END) };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as u64)
    }
}

/// The whole test on `dev` (runs as root). Returns false when cancelled.
fn core(dev: &str, state: &Shared, cancel: &AtomicBool) -> io::Result<bool> {
    use std::os::unix::fs::OpenOptionsExt;
    if mounted(dev) {
        return Err(io::Error::other("the drive is mounted"));
    }
    let file = std::fs::OpenOptions::new().read(true).write(true).custom_flags(libc::O_DIRECT | libc::O_SYNC).open(dev)?;
    let size = size_of(&file)?;
    let cfg = Config { dir: PathBuf::new(), bytes: size / BLOCK as u64 * BLOCK as u64, quick: true };
    let plan = fakecheck::plan(&cfg);
    if plan.len() < 8 {
        return Err(io::Error::other("the drive is too small to test"));
    }
    let n = plan.len();
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
    update(state, |s| {
        s.total = n as u64;
        s.direct = true;
        s.phase = Phase::Writing;
    });
    let at = |i: u64| i * BLOCK as u64;
    let mut buf = bench_buf();
    // 1. Save what is there.
    let mut originals = vec![0u8; n * CHUNK];
    for (k, &i) in plan.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(false);
        }
        crate::bench::read_full(&file, &mut buf.as_mut_slice()[..CHUNK], at(i))?;
        originals[k * CHUNK..(k + 1) * CHUNK].copy_from_slice(&buf.as_slice()[..CHUNK]);
        if k % 8 == 0 {
            update(state, |s| s.fraction = 0.15 * k as f32 / n as f32);
        }
    }
    // 2. Write, 3. read back; whatever happens, 4. put everything back.
    let mut written = 0usize;
    let result = (|| -> io::Result<bool> {
        let t0 = std::time::Instant::now();
        for (k, &i) in plan.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(false);
            }
            fakecheck::fill_block(&mut buf.as_mut_slice()[..CHUNK], seed, i);
            written = k + 1; // counted before the write: a half-done write is put back too
            crate::bench::write_full(&file, &buf.as_slice()[..CHUNK], at(i))?;
            if k % 4 == 0 || k + 1 == n {
                update(state, |s| {
                    s.written = k as u64 + 1;
                    s.fraction = 0.15 + 0.35 * (k + 1) as f32 / n as f32;
                });
            }
        }
        let secs = t0.elapsed().as_secs_f64().max(1e-6);
        update(state, |s| {
            s.write_avg = (n * CHUNK) as f64 / secs / 1e6;
            s.write_min = s.write_avg;
            s.phase = Phase::Verifying;
        });
        let t1 = std::time::Instant::now();
        let mut expected = bench_buf();
        for (k, &i) in plan.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(false);
            }
            let read = crate::bench::read_full(&file, &mut buf.as_mut_slice()[..CHUNK], at(i));
            fakecheck::fill_block(&mut expected.as_mut_slice()[..CHUNK], seed, i);
            let kind = match read {
                Ok(()) if buf.as_slice()[..CHUNK] == expected.as_slice()[..CHUNK] => None,
                Ok(()) => Some(fakecheck::classify(&buf.as_slice()[..CHUNK], seed, i)),
                Err(_) => Some(BlockKind::Empty),
            };
            update(state, |s| {
                s.verified = k as u64 + 1;
                s.fraction = 0.5 + 0.45 * (k + 1) as f32 / n as f32;
                if let Some(kind) = kind {
                    s.bad_count += 1;
                    if s.bad.len() < 40 {
                        s.bad.push(BadBlock { index: i, kind });
                    }
                }
            });
        }
        let secs = t1.elapsed().as_secs_f64().max(1e-6);
        update(state, |s| {
            s.read_avg = (n * CHUNK) as f64 / secs / 1e6;
            s.read_min = s.read_avg;
        });
        Ok(true)
    })();
    // 4. Put back what was there.
    let mut restore_failed = false;
    for (k, &i) in plan.iter().enumerate().take(written) {
        buf.as_mut_slice()[..CHUNK].copy_from_slice(&originals[k * CHUNK..(k + 1) * CHUNK]);
        if crate::bench::write_full(&file, &buf.as_slice()[..CHUNK], at(i)).is_err() {
            restore_failed = true;
        }
    }
    let ok = result?;
    if restore_failed {
        return Err(io::Error::other("could not put back the original data of some spots"));
    }
    Ok(ok)
}

fn bench_buf() -> crate::bench::AlignedBuf {
    crate::bench::AlignedBuf::new(CHUNK)
}

fn finish(state: &Shared, result: io::Result<bool>) {
    update(state, |s| match result {
        Ok(true) => {
            s.phase = Phase::Done;
            s.fraction = 1.0;
            s.verdict = Some(fakecheck::judge(s));
        }
        Ok(false) => s.phase = Phase::Cancelled,
        Err(e) => s.phase = Phase::Failed(e.to_string()),
    });
}

/// `diskinfo rawcheck /dev/sdX --cancel-file F`: what runs as root. Prints a line with the state
/// as JSON now and then, and a last one when it ends.
pub fn helper_main(args: &[String]) -> i32 {
    let dev = args.get(2).cloned().unwrap_or_default();
    let valid = dev.strip_prefix("/dev/").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric()));
    if !valid {
        eprintln!("usage: diskinfo rawcheck /dev/<drive> [--cancel-file FILE]");
        return 2;
    }
    let cancel_file = args.iter().position(|a| a == "--cancel-file").and_then(|i| args.get(i + 1)).cloned();
    let state: Shared = Arc::new(Mutex::new(State::new(&Config { dir: PathBuf::new(), bytes: 0, quick: true })));
    // Already "writing": a first line saying "idle" would look like a finished test to the window.
    update(&state, |s| s.phase = Phase::Writing);
    let cancel = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    // Watches for the cancel file and reports the state.
    let watcher = {
        let (state, cancel, done) = (state.clone(), cancel.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                if cancel_file.as_deref().is_some_and(|f| Path::new(f).exists()) {
                    cancel.store(true, Ordering::Relaxed);
                }
                let line = serde_json::to_string(&*state.lock().unwrap_or_else(|e| e.into_inner())).unwrap_or_default();
                println!("{PREFIX}{line}");
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        })
    };
    let result = core(&dev, &state, &cancel);
    let failed = result.is_err();
    finish(&state, result);
    done.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    let line = serde_json::to_string(&*state.lock().unwrap_or_else(|e| e.into_inner())).unwrap_or_default();
    println!("{PREFIX}{line}");
    i32::from(failed)
}

// ------------------------------------------------------------------------- the window's side

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Copies this program where root can read it (the AppImage's own mount is private to the user).
fn stage_self() -> Option<(String, String)> {
    use std::os::unix::fs::PermissionsExt;
    let bytes = std::fs::read(std::env::current_exe().ok()?).ok()?;
    let hash = crate::smartctl::sha256_hex(&bytes);
    let dir = crate::platform::data_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let dst = dir.join("diskinfo-helper");
    if std::fs::read(&dst).ok().as_deref() != Some(bytes.as_slice()) {
        std::fs::write(&dst, &bytes).ok()?;
    }
    let _ = std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755));
    Some((dst.to_string_lossy().into_owned(), hash))
}

fn run_as_root(dev: &str, state: &Shared, cancel: &Arc<AtomicBool>) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let valid = dev.strip_prefix("/dev/").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric()));
    if !valid {
        return Err("not a drive".into());
    }
    let (src, hash) = stage_self().ok_or("could not prepare the helper")?;
    let cancel_file = crate::platform::data_dir().ok_or("no data folder")?.join("raw-cancel");
    let _ = std::fs::remove_file(&cancel_file);
    let script = format!(
        "H=$(mktemp /run/diskinfo-helper.XXXXXX) || exit 9; trap 'rm -f \"$H\"' EXIT; \
         cat {src} > \"$H\" && chmod 700 \"$H\" && echo \"{hash}  $H\" | sha256sum -c --quiet - || exit 9; \
         \"$H\" rawcheck {dev} --cancel-file {cf}",
        src = shell_quote(&src),
        dev = shell_quote(dev),
        cf = shell_quote(&cancel_file.to_string_lossy()),
    );
    let mut child = Command::new("pkexec")
        .args(["/bin/sh", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "pkexec is missing".to_string())?;
    let finished = Arc::new(AtomicBool::new(false));
    let stopper = {
        let (cancel, finished, cf) = (cancel.clone(), finished.clone(), cancel_file.clone());
        std::thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                if cancel.load(Ordering::Relaxed) {
                    let _ = std::fs::write(&cf, b"stop");
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        })
    };
    let mut last: Option<State> = None;
    if let Some(out) = child.stdout.take() {
        for line in io::BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(json) = line.strip_prefix(PREFIX) {
                if let Ok(s) = serde_json::from_str::<State>(json) {
                    update(state, |cur| *cur = s.clone());
                    last = Some(s);
                }
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let mut err_text = String::new();
    if let Some(mut e) = child.stderr.take() {
        use std::io::Read;
        let _ = e.read_to_string(&mut err_text);
    }
    let err_line = err_text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
    finished.store(true, Ordering::Relaxed);
    let _ = stopper.join();
    let _ = std::fs::remove_file(&cancel_file);
    match last {
        Some(s) if !s.running() => Ok(()),
        _ if matches!(status.code(), Some(126) | Some(127)) => Err(format!("administrator rights were not granted {err_line}").trim().to_string()),
        _ => Err(format!("the test stopped before it finished (code {}) {err_line}", status.code().map_or("?".to_string(), |c| c.to_string())).trim().to_string()),
    }
}

/// Starts the quick test on the raw drive `dev` ("/dev/sdb"); same handle as the file tests.
pub fn start(dev: String) -> Handle {
    let state: Shared = Arc::new(Mutex::new(State::new(&Config { dir: PathBuf::new(), bytes: 0, quick: true })));
    update(&state, |s| s.phase = Phase::Writing);
    let cancel = Arc::new(AtomicBool::new(false));
    let (s, c) = (state.clone(), cancel.clone());
    std::thread::spawn(move || {
        if let Err(e) = run_as_root(&dev, &s, &c) {
            update(&s, |s| s.phase = Phase::Failed(e));
        }
    });
    Handle { state, cancel }
}

/// Short parse check used by tests.
#[allow(dead_code)]
fn parse_state(line: &str) -> Option<Value> {
    serde_json::from_str(line.strip_prefix(PREFIX)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_survives_the_trip_through_json() {
        let mut s = State::new(&Config { dir: PathBuf::new(), bytes: 100 << 20, quick: true });
        s.phase = Phase::Failed("x".into());
        s.bad.push(BadBlock { index: 3, kind: BlockKind::Overwritten(9) });
        s.verdict = Some(fakecheck::Verdict::Fake { real_bytes: 5 });
        let line = format!("{PREFIX}{}", serde_json::to_string(&s).unwrap());
        assert!(parse_state(&line).is_some());
        let back: State = serde_json::from_str(line.strip_prefix(PREFIX).unwrap()).unwrap();
        assert_eq!(back.bad[0].kind, BlockKind::Overwritten(9));
        assert_eq!(back.verdict, Some(fakecheck::Verdict::Fake { real_bytes: 5 }));
    }

    #[test]
    fn a_good_drive_passes_and_its_data_is_put_back() {
        use std::io::Write;
        // A plain file on a disk file system stands in for the drive (tmpfs has no direct I/O).
        let Some(home) = std::env::var_os("HOME") else { return };
        let path = PathBuf::from(home).join(".cache").join(format!("diskinfo-raw-test-{}", std::process::id()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let size = 300usize << 20;
        let mut data = vec![0u8; size];
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i.wrapping_mul(2654435761) >> 13) as u8;
        }
        std::fs::File::create(&path).unwrap().write_all(&data).unwrap();
        let state: Shared = Arc::new(Mutex::new(State::new(&Config { dir: PathBuf::new(), bytes: 0, quick: true })));
        let cancel = AtomicBool::new(false);
        let r = core(path.to_str().unwrap(), &state, &cancel);
        let after = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        match r {
            Err(e) if e.raw_os_error() == Some(22) => return, // no direct I/O here
            r => assert!(r.unwrap()),
        }
        finish(&state, Ok(true));
        let s = state.lock().unwrap().clone();
        assert_eq!(s.bad_count, 0);
        assert_eq!(s.verdict, Some(fakecheck::Verdict::Genuine));
        assert!(after == data, "the original data must be back");
    }

    #[test]
    fn a_fresh_state_does_not_look_finished() {
        // The window drops its handle as soon as a state says "not running".
        let s = State::new(&Config { dir: PathBuf::new(), bytes: 0, quick: true });
        assert!(s.running());
    }

    #[test]
    fn refuses_bad_names() {
        assert_eq!(helper_main(&["d".into(), "rawcheck".into(), "/dev/sda; rm".into()]), 2);
        assert_eq!(helper_main(&["d".into(), "rawcheck".into(), "sda".into()]), 2);
    }
}
