//! Keeping the server answering when much is open, and saying what is stuck
//! when it doesn't: a stall used to leave no trace once the app restarted.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;

const CHECK_EVERY: Duration = Duration::from_secs(1);
/// How long a probe task may wait for a worker before the runtime counts as
/// stalled: a fresh task normally runs within a millisecond, so a second is a
/// worker blocked on something it should have handed to the blocking pool.
const STALL: Duration = Duration::from_secs(1);
/// How far the watchdog's own sleep may overrun before the whole process counts
/// as frozen: no thread of it ran, so neither probe could notice.
const FROZE: Duration = Duration::from_secs(3);
/// At most one thread sample per this long, and this many kept.
const SAMPLE_EVERY: Duration = Duration::from_secs(600);
const SAMPLES_KEPT: usize = 10;
/// Longer than any request waits on purpose (long polls 20s, chat starts 30s).
const SLOW_REQUEST: Duration = Duration::from_secs(45);

/// Raise the soft open-file limit as far as the hard limit allows; once, at
/// process start, before any server runs. An app started by launchd gets 256,
/// which a few terminals (three descriptors each) and sockets use up; past it
/// every `accept` fails and the API stops answering.
///
/// Shells, `claude` and `codex` started afterwards inherit the raised limit.
/// That's left as is: restoring 256 in them would need `pre_exec` (which makes
/// std fork this many-threaded process, see `workbench_core::pty`) or a
/// wrapper around every launch, and a raised soft limit is what Node-based
/// tools (Claude Code included) and most terminal apps already give their
/// children. The cap keeps a child that closes every possible descriptor cheap.
#[cfg(unix)]
pub fn raise_fd_limit() {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return;
    }
    // macOS refuses more than kern.maxfilesperproc (and RLIM_INFINITY).
    let mut want = limit.rlim_max.min(1 << 16);
    while want > limit.rlim_cur {
        let raised = libc::rlimit {
            rlim_cur: want,
            ..limit
        };
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raised) } == 0 {
            tracing::info!("open-file limit raised from {} to {want}", limit.rlim_cur);
            return;
        }
        want /= 2;
    }
}

#[cfg(not(unix))]
pub fn raise_fd_limit() {}

/// Watch, until the runtime shuts down, that its async workers and blocking
/// pool still run work and that open files stay clear of the limit. Called by
/// whoever builds the runtime the server runs on.
pub fn start(handle: tokio::runtime::Handle) {
    let _ = std::thread::Builder::new()
        .name("workbench-server-watchdog".into())
        .spawn(move || watch(handle));
}

fn watch(handle: tokio::runtime::Handle) {
    let mut files_low = false;
    let mut sampled = None;
    loop {
        let slept = Instant::now();
        std::thread::sleep(CHECK_EVERY);
        note_freeze(slept.elapsed().saturating_sub(CHECK_EVERY));
        let async_ran = probe("async workers", &mut sampled, |done| {
            handle.spawn(async move { done.send(()) });
        });
        let blocking_ran = probe("blocking pool", &mut sampled, |done| {
            handle.spawn_blocking(move || done.send(()));
        });
        if !async_ran || !blocking_ran {
            return; // the runtime shut down
        }
        files_low = check_files(files_low);
    }
}

/// Report a sleep that overran by `late`: the whole process stopped (memory
/// pressure swapping it out, a fork holding the allocator's locks), which no
/// probe can see from inside. Unix monotonic clocks stop while the machine
/// sleeps, so a sleeping laptop isn't reported; Windows' may not, so it's skipped.
fn note_freeze(late: Duration) {
    if cfg!(windows) || late < FROZE {
        return;
    }
    // A breadcrumb first: Sentry attaches it to the error that follows.
    tracing::warn!(
        "no thread of the process ran for {:.1}s{}",
        late.as_secs_f64(),
        memory_note()
    );
    tracing::error!(
        "process froze: the server watchdog ran over {}s late",
        FROZE.as_secs()
    );
}

/// Log when `start`'s task waits past [`STALL`] for a thread, and again when
/// it runs. False once the runtime is gone.
fn probe(what: &str, sampled: &mut Option<Instant>, start: impl FnOnce(mpsc::Sender<()>)) -> bool {
    let (done, ran) = mpsc::channel();
    let started = Instant::now();
    start(done);
    match ran.recv_timeout(STALL) {
        Ok(()) => return true,
        Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        Err(mpsc::RecvTimeoutError::Timeout) => {}
    }
    sample_threads(sampled);
    tracing::error!(
        "server {what} stalled: a task waited over {}s to run",
        STALL.as_secs()
    );
    let alive = ran.recv().is_ok();
    tracing::warn!(
        "server {what} ran again after {:.1}s",
        started.elapsed().as_secs_f64()
    );
    alive
}

/// Every thread's stack while a stall lasts, which says what the workers wait
/// on: macOS's `sample` writes them to `logs/stall-<unix secs>.txt` beside the
/// log, at most once per [`SAMPLE_EVERY`], keeping the newest [`SAMPLES_KEPT`].
#[cfg(target_os = "macos")]
fn sample_threads(last: &mut Option<Instant>) {
    if last.is_some_and(|at| at.elapsed() < SAMPLE_EVERY) {
        return;
    }
    *last = Some(Instant::now());
    let dir = workbench_core::paths::workbench_config_dir().join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    prune_samples(&dir);
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let file = dir.join(format!("stall-{secs}.txt"));
    let started = workbench_core::shell::spawn_detached(
        workbench_core::shell::command("/usr/bin/sample")
            .arg(std::process::id().to_string())
            .args(["3", "-file"])
            .arg(&file)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null()),
    );
    match started {
        Ok(()) => tracing::warn!("sampling every thread to {}", file.display()),
        Err(e) => tracing::warn!("couldn't sample threads: {e}"),
    }
}

#[cfg(not(target_os = "macos"))]
fn sample_threads(_: &mut Option<Instant>) {}

/// Leave room for one more sample among the newest [`SAMPLES_KEPT`].
#[cfg(target_os = "macos")]
fn prune_samples(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut samples: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("stall-") && n.ends_with(".txt"))
        })
        .collect();
    // Same-length unix seconds, so the names sort by age.
    samples.sort();
    let over = (samples.len() + 1).saturating_sub(SAMPLES_KEPT);
    for old in &samples[..over] {
        let _ = std::fs::remove_file(old);
    }
}

/// `; swap 5837 of 6144 MB used, memory pressure warn` on macOS, where a
/// frozen process is most often one swapped out.
#[cfg(target_os = "macos")]
fn memory_note() -> String {
    /// `T` is a plain C value (an int, `xsw_usage`), valid all zeroes.
    fn read<T: Copy>(name: &std::ffi::CStr) -> Option<T> {
        let mut value: T = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<T>();
        let ok = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                (&mut value as *mut T).cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        } == 0;
        ok.then_some(value)
    }
    let mut note = String::new();
    if let Some(swap) = read::<libc::xsw_usage>(c"vm.swapusage") {
        let mb = |b: u64| b / (1024 * 1024);
        note += &format!(
            "; swap {} of {} MB used",
            mb(swap.xsu_used),
            mb(swap.xsu_total)
        );
    }
    if let Some(level) = read::<libc::c_int>(c"kern.memorystatus_vm_pressure_level") {
        let level = match level {
            1 => "normal",
            2 => "warn",
            4 => "critical",
            _ => "unknown",
        };
        note += &format!(", memory pressure {level}");
    }
    note
}

#[cfg(not(target_os = "macos"))]
fn memory_note() -> String {
    String::new()
}

/// Log once when open files pass 80% of the limit; whether they still do.
#[cfg(unix)]
fn check_files(was_low: bool) -> bool {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0
        || limit.rlim_cur == libc::RLIM_INFINITY
    {
        return was_low;
    }
    let open = match std::fs::read_dir("/dev/fd") {
        Ok(entries) => entries.count() as u64,
        Err(e) => {
            if !was_low {
                tracing::error!("can't count open files ({e}): out of descriptors?");
            }
            return true;
        }
    };
    let low = open.saturating_mul(5) > limit.rlim_cur.saturating_mul(4);
    if low && !was_low {
        tracing::error!(
            "{open} of {} file descriptors open: new connections will soon fail",
            limit.rlim_cur
        );
    }
    low
}

#[cfg(not(unix))]
fn check_files(was_low: bool) -> bool {
    was_low
}

/// Name a request still unanswered after [`SLOW_REQUEST`], and when it is.
pub async fn slow_requests(request: Request, next: Next) -> Response {
    let route = request.extensions().get::<MatchedPath>().cloned();
    let method = request.method().clone();
    let started = Instant::now();
    let run = next.run(request);
    tokio::pin!(run);
    if let Ok(response) = tokio::time::timeout(SLOW_REQUEST, &mut run).await {
        return response;
    }
    let what = format!(
        "{method} {}",
        route.as_ref().map_or("unmatched", |p| p.as_str())
    );
    tracing::warn!("{what} unanswered after {}s", SLOW_REQUEST.as_secs());
    let response = run.await;
    tracing::warn!(
        "{what} answered after {:.1}s",
        started.elapsed().as_secs_f64()
    );
    response
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn pruning_keeps_room_for_one_more_among_the_newest_samples() {
        let dir = tempfile::tempdir().unwrap();
        for secs in 1_700_000_000..1_700_000_012u64 {
            std::fs::write(dir.path().join(format!("stall-{secs}.txt")), "").unwrap();
        }
        std::fs::write(dir.path().join("workbench.log"), "").unwrap();
        prune_samples(dir.path());
        let mut left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left.len(), SAMPLES_KEPT, "9 samples and the log");
        assert_eq!(left[0], "stall-1700000003.txt");
        assert!(left.contains(&"workbench.log".to_string()));
    }

    #[test]
    fn the_memory_note_reads_swap_and_pressure() {
        let note = memory_note();
        assert!(note.starts_with("; swap "), "{note}");
        assert!(note.contains(", memory pressure "), "{note}");
    }
}
