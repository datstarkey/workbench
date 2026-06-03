//! Backend (Rust) error tracking + log forwarding via Sentry.
//!
//! Mirrors the frontend setup in `src/lib/sentry.ts`:
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
//! A global `log` logger is installed in **all** builds, writing to stderr via
//! `env_logger` (respects `RUST_LOG`, defaults to `info`). In release builds it
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
    let dest =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).build();
    let max_level = dest.filter();

    let logger = SentryLogger::with_dest(dest).filter(|metadata| match metadata.level() {
        log::Level::Error => LogFilter::Event,
        _ => LogFilter::Breadcrumb,
    });

    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(max_level);
    }
}
