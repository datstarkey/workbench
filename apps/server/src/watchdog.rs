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
