//! Keeping the server answering when much is open, and saying what is stuck
//! when it doesn't: a stall used to leave no trace once the app restarted.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;

const CHECK_EVERY: Duration = Duration::from_secs(5);
/// How long a probe task may wait for a worker before the runtime counts as stalled.
const STALL: Duration = Duration::from_secs(5);
/// Longer than any request waits on purpose (long polls 20s, chat starts 30s).
const SLOW_REQUEST: Duration = Duration::from_secs(45);

/// Raise the soft open-file limit as far as the hard limit allows. An app
/// started by launchd gets 256, which a few terminals (three descriptors
/// each) and sockets use up; past it every `accept` fails and the API stops
/// answering.
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

/// Watch, once per process, that the runtime's async workers and blocking
/// pool still run work and that open files stay clear of the limit.
pub fn spawn(handle: tokio::runtime::Handle) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("workbench-server-watchdog".into())
            .spawn(move || watch(handle));
    });
}

fn watch(handle: tokio::runtime::Handle) {
    let mut files_low = false;
    loop {
        std::thread::sleep(CHECK_EVERY);
        let async_ran = probe("async workers", |done| {
            handle.spawn(async move { done.send(()) });
        });
        let blocking_ran = probe("blocking pool", |done| {
            handle.spawn_blocking(move || done.send(()));
        });
        if !async_ran || !blocking_ran {
            return; // the runtime shut down
        }
        files_low = check_files(files_low);
    }
}

/// Log when `start`'s task waits past [`STALL`] for a thread, and again when
/// it runs. False once the runtime is gone.
fn probe(what: &str, start: impl FnOnce(mpsc::Sender<()>)) -> bool {
    let (done, ran) = mpsc::channel();
    let started = Instant::now();
    start(done);
    match ran.recv_timeout(STALL) {
        Ok(()) => return true,
        Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        Err(mpsc::RecvTimeoutError::Timeout) => {}
    }
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

/// Log once when open files pass 80% of the limit; whether they still do.
#[cfg(unix)]
fn check_files(was_low: bool) -> bool {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
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
    let low = open * 5 > limit.rlim_cur * 4;
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
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", |p| p.as_str())
        .to_string();
    let what = format!("{} {route}", request.method());
    let started = Instant::now();
    let run = next.run(request);
    tokio::pin!(run);
    if let Ok(response) = tokio::time::timeout(SLOW_REQUEST, &mut run).await {
        return response;
    }
    tracing::warn!("{what} unanswered after {}s", SLOW_REQUEST.as_secs());
    let response = run.await;
    tracing::warn!(
        "{what} answered after {:.1}s",
        started.elapsed().as_secs_f64()
    );
    response
}
