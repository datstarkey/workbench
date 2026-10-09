//! Backend (Rust) error tracking + log forwarding via Sentry.
//!
//! Mirrors the frontend setup in `apps/desktop/src/lib/sentry.ts`:
//! - Self-hosted Sentry at `sentry.starkeydigital.com`, project `workbench`.
//! - **Errors only** — no performance/session tracing.
//! - **Prod-gated:** the Sentry client is a no-op in debug builds
//!   (`tauri dev`), so local development never reports upstream.
//!
//! The frontend captures JS errors and rejected `invoke()` calls (the error
//! string crosses IPC back to JS). This module covers the gap: Rust panics
//! (including background reader/watcher threads) and the backend log trail
//! (`error!` events + `warn!`/`info!` breadcrumbs).
//!
//! ## Logging
//!
//! A global `log` logger is installed in **all** builds, writing to stderr and
//! `<config dir>/logs/workbench.log` (one `.old` copy past 5 MB) via `env_logger` (respects `RUST_LOG`,
//! defaults to `info`). The server's `tracing` events arrive through
//! tracing's `log` feature. In release builds it
//! additionally forwards through Sentry:
//! - `error!` → captured as a standalone Sentry **event** (an issue).
//! - `warn!` / `info!` → recorded as **breadcrumbs**, attached to whatever
//!   event fires next, so each issue shows the log trail leading up to it.
//!
//! In debug builds the Sentry client is inactive, so forwarding is a no-op and
//! only the stderr output remains.

use sentry::integrations::log::{LogFilter, SentryLogger};

/// Public ingest DSN (write-only key). Overridable via `SENTRY_DSN` at build
/// time. Shares the same project as the frontend SDK.
const DSN: &str = "https://708b68ab317b3d17816c9f8135337d11@sentry.starkeydigital.com/20";

/// Initialise backend logging + Sentry. Returns a guard that must be kept alive
/// for the duration of the process (drop flushes pending events). Returns
/// `None` in debug builds, where upstream reporting is disabled — stderr
/// logging still works.
///
/// `release` should be the app version (e.g. `"0.22.0"`); it is tagged as
/// `workbench@<release>` to match the frontend release convention.
#[must_use]
pub fn init(release: String) -> Option<sentry::ClientInitGuard> {
    // Install the logger in every build so stderr output and (in release)
    // Sentry breadcrumbs/events are populated.
    init_logger();

    // Prod-gate: never report from `tauri dev` / debug builds, or from the
    // test harness (even a `--release` test run).
    if cfg!(debug_assertions) || cfg!(test) {
        return None;
    }

    let dsn = option_env!("SENTRY_DSN").unwrap_or(DSN);
    if dsn.is_empty() {
        return None;
    }

    let guard = sentry::init((
        dsn,
        sentry::ClientOptions {
            release: Some(format!("workbench@{release}").into()),
            environment: Some("production".into()),
            // Errors only — no performance/session tracing for a desktop app.
            traces_sample_rate: 0.0,
            ..Default::default()
        },
    ));

    Some(guard)
}

/// Install a global logger: env_logger to stderr, wrapped by Sentry so `error!`
/// becomes an event and `warn!`/`info!` become breadcrumbs. Safe to call once;
/// a second call (e.g. in tests) is ignored.
fn init_logger() {
    let dest = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Pipe(Box::new(Tee(log_file()))))
        .build();
    let max_level = dest.filter();

    let logger = SentryLogger::with_dest(dest).filter(|metadata| match metadata.level() {
        log::Level::Error => LogFilter::Event,
        _ => LogFilter::Breadcrumb,
    });

    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(max_level);
    }
}

/// Kept past a restart: a stall that ran the process out of sockets can't
/// reach Sentry either.
const LOG_FILE: &str = "workbench.log";
/// A bigger log becomes the one previous copy.
const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;

/// The log file, unbuffered, and how much it holds.
struct LogFile {
    path: std::path::PathBuf,
    file: std::fs::File,
    len: u64,
}

impl LogFile {
    fn open(path: std::path::PathBuf) -> Option<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        let len = file.metadata().map_or(0, |m| m.len());
        Some(Self { path, file, len })
    }

    fn append(&mut self, buf: &[u8]) {
        use std::io::Write;
        if self.len > LOG_ROTATE_BYTES {
            let mut old = self.path.clone().into_os_string();
            old.push(".old");
            let _ = std::fs::rename(&self.path, old);
            if let Some(fresh) = Self::open(self.path.clone()) {
                *self = fresh;
            }
        }
        if self.file.write_all(buf).is_ok() {
            self.len += buf.len() as u64;
        }
    }
}

fn log_file() -> Option<LogFile> {
    let dir = workbench_core::paths::workbench_config_dir().join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    LogFile::open(dir.join(LOG_FILE))
}

/// Every record to stderr and to the log file.
struct Tee(Option<LogFile>);

impl std::io::Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(file) = &mut self.0 {
            file.append(buf);
        }
        std::io::stderr().write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stderr().flush()
    }
}
