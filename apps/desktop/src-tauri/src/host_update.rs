//! Updating this app: one check, one install and one guard for the desktop's own
//! `UpdaterStore` (`host_update_status`/`host_update_install`) and a paired phone
//! (`/host/update`) alike. The updater verifies each release's signature, so a
//! token holder can only install a genuine Workbench release.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};
use workbench_core::types::{HostUpdateStarted, HostUpdateStatus, UpdateOrigin};
use workbench_server::host::{BoxFuture, HostControl, Install};

/// The phone polls `GET /host/update`; one feed check answers it (and its install) this long.
const FEED_TTL: Duration = Duration::from_secs(5 * 60);
/// `update:progress` is sent at most this often, and once more when the download ends.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);
/// Written in the config dir just before the installer runs, so the relaunch
/// (which keeps a login launch's `--autostart`) isn't minimised like a login.
const RESTART_MARKER: &str = "update-restart";

/// `update:installing`.
#[derive(Clone, Serialize)]
struct Installing {
    version: String,
    origin: UpdateOrigin,
}

/// `update:progress`: the running install's download, whoever started it.
#[derive(Clone, Serialize)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// `update:failed`.
#[derive(Clone, Serialize)]
struct Failed {
    error: String,
    origin: UpdateOrigin,
}

pub struct DesktopHost<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
    feed: Mutex<Option<(Instant, Option<Update>)>>,
}

impl<R: Runtime> DesktopHost<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            feed: Mutex::new(None),
        }
    }

    async fn available(&self, fresh: bool) -> anyhow::Result<Option<Update>> {
        if let (false, Some((at, update))) = (fresh, &*lock(&self.feed)) {
            if at.elapsed() < FEED_TTL {
                return Ok(update.clone());
            }
        }
        let update = self.app.updater()?.check().await?;
        *lock(&self.feed) = Some((Instant::now(), update.clone()));
        Ok(update)
    }
}

impl<R: Runtime> HostControl for DesktopHost<R> {
    fn check(&self, fresh: bool) -> BoxFuture<'_, anyhow::Result<HostUpdateStatus>> {
        Box::pin(async move {
            let current = self.app.package_info().version.to_string();
            if let Some((version, origin)) = self.app.state::<UpdateGuard>().installing() {
                return Ok(HostUpdateStatus {
                    current,
                    available: Some(version),
                    body: None,
                    installing: true,
                    started_by: Some(origin),
                });
            }
            let update = self.available(fresh).await?;
            Ok(HostUpdateStatus {
                current,
                available: update.as_ref().map(|u| u.version.clone()),
                body: update.and_then(|u| u.body),
                installing: false,
                started_by: None,
            })
        })
    }

    fn install(
        &self,
        version: Option<String>,
        origin: UpdateOrigin,
    ) -> BoxFuture<'_, anyhow::Result<Install>> {
        Box::pin(async move {
            let guard = self.app.state::<UpdateGuard>();
            if let Some((running, _)) = guard.installing() {
                return Ok(Install::Started(running));
            }
            let reviewed = |update: &Option<Update>| match (&version, update) {
                (Some(want), Some(u)) => &u.version == want,
                (Some(_), None) => false,
                (None, _) => true,
            };
            let mut update = self.available(false).await?;
            if !reviewed(&update) {
                update = self.available(true).await?;
            }
            let Some(update) = update else {
                return Ok(Install::Unavailable);
            };
            if version.as_ref().is_some_and(|want| *want != update.version) {
                return Ok(Install::Changed(update.version));
            }
            let version = update.version.clone();
            if let Err(running) = guard.begin(&version, origin) {
                return Ok(Install::Started(running));
            }
            let lease = Lease(self.app.clone());
            let _ = self.app.emit(
                "update:installing",
                Installing {
                    version: version.clone(),
                    origin,
                },
            );
            let app = self.app.clone();
            tauri::async_runtime::spawn(async move {
                let _lease = lease;
                if let Err(e) = install(&app, update).await {
                    log::error!("update install failed: {e:#}");
                    let error = format!("{e:#}");
                    let _ = app.emit("update:failed", Failed { error, origin });
                }
            });
            Ok(Install::Started(version))
        })
    }
}

/// Download while sessions keep running, then end them before installing (on
/// Windows the installer exits the app, and children outliving it keep the old
/// Dock tile or console window).
async fn install<R: Runtime>(app: &AppHandle<R>, update: Update) -> anyhow::Result<()> {
    log::warn!("installing Workbench {}", update.version);
    let (mut downloaded, mut total, mut sent) = (0u64, None, None::<Instant>);
    let bytes = update
        .download(
            |chunk, length| {
                downloaded += chunk as u64;
                total = length;
                if sent.is_none_or(|at| at.elapsed() >= PROGRESS_EVERY) {
                    sent = Some(Instant::now());
                    let _ = app.emit("update:progress", Progress { downloaded, total });
                }
            },
            || {},
        )
        .await?;
    let _ = app.emit("update:progress", Progress { downloaded, total });
    crate::server_control::kill_all_sessions(&app.state::<crate::server_control::ServerControl>())
        .await;
    let dir = crate::paths::workbench_config_dir();
    if let Err(e) = write_restart_marker(&dir) {
        log::warn!("couldn't mark the update restart: {e:#}");
    }
    let installed = tauri::async_runtime::spawn_blocking(move || update.install(bytes)).await;
    if !matches!(installed, Ok(Ok(()))) {
        remove_restart_marker(&dir);
    }
    installed??;
    app.restart();
}

fn restart_marker(dir: &Path) -> PathBuf {
    dir.join(RESTART_MARKER)
}

fn unix_now() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

fn write_restart_marker(dir: &Path) -> anyhow::Result<()> {
    let now = unix_now().as_secs().to_string();
    crate::paths::atomic_write(&restart_marker(dir), &now)
}

/// How long ago an install wrote the marker; an unreadable one counts as stale.
pub fn restart_marker_age(dir: &Path) -> Option<Duration> {
    let text = std::fs::read_to_string(restart_marker(dir)).ok()?;
    Some(match text.trim().parse::<u64>() {
        Ok(at) => unix_now().saturating_sub(Duration::from_secs(at)),
        Err(_) => Duration::MAX,
    })
}

pub fn remove_restart_marker(dir: &Path) {
    let _ = std::fs::remove_file(restart_marker(dir));
}

/// One install at a time, whichever side started it. Holds the version being
/// installed and who started it.
#[derive(Default)]
pub struct UpdateGuard(Mutex<Option<(String, UpdateOrigin)>>);

impl UpdateGuard {
    /// Claim the install for `version`; `Err` names the version already installing.
    fn begin(&self, version: &str, origin: UpdateOrigin) -> Result<(), String> {
        let mut slot = lock(&self.0);
        match &*slot {
            Some((running, _)) => Err(running.clone()),
            None => {
                *slot = Some((version.to_string(), origin));
                Ok(())
            }
        }
    }

    fn end(&self) {
        *lock(&self.0) = None;
    }

    fn installing(&self) -> Option<(String, UpdateOrigin)> {
        lock(&self.0).clone()
    }
}

/// Ends an install's claim when it fails, so it can be retried.
struct Lease<R: Runtime>(AppHandle<R>);

impl<R: Runtime> Drop for Lease<R> {
    fn drop(&mut self) {
        self.0.state::<UpdateGuard>().end();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `GET /host/update` over IPC.
#[tauri::command]
pub async fn host_update_status(
    host: tauri::State<'_, Arc<DesktopHost>>,
    fresh: Option<bool>,
) -> Result<HostUpdateStatus, String> {
    host.check(fresh.unwrap_or(false))
        .await
        .map_err(|e| format!("{e:#}"))
}

/// `POST /host/update` over IPC; progress and failure arrive as `update:*` events.
#[tauri::command]
pub async fn host_update_install(
    host: tauri::State<'_, Arc<DesktopHost>>,
    version: Option<String>,
) -> Result<HostUpdateStarted, String> {
    host.install(version, UpdateOrigin::Desktop)
        .await
        .map_err(|e| format!("{e:#}"))?
        .started()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[test]
    fn only_one_install_runs_until_it_ends() {
        let guard = UpdateGuard::default();
        guard
            .begin("1.1.0", UpdateOrigin::Remote)
            .expect("first install starts");
        assert_eq!(
            guard.installing(),
            Some(("1.1.0".to_string(), UpdateOrigin::Remote))
        );
        assert_eq!(
            guard.begin("1.1.0", UpdateOrigin::Desktop),
            Err("1.1.0".to_string())
        );

        guard.end();
        assert_eq!(guard.installing(), None);
        assert!(
            guard.begin("1.2.0", UpdateOrigin::Desktop).is_ok(),
            "a failed install can be retried"
        );
    }

    #[tokio::test]
    async fn a_second_install_joins_the_running_one_for_every_caller() {
        let app = mock_builder()
            .manage(UpdateGuard::default())
            .build(mock_context(noop_assets()))
            .expect("mock app");
        let host = DesktopHost::new(app.handle().clone());
        app.state::<UpdateGuard>()
            .begin("9.9.9", UpdateOrigin::Remote)
            .expect("a phone's install");

        let status = host.check(true).await.expect("status");
        assert!(status.installing);
        assert_eq!(status.available.as_deref(), Some("9.9.9"));
        assert_eq!(status.started_by, Some(UpdateOrigin::Remote));

        let joined = host
            .install(Some("9.9.9".into()), UpdateOrigin::Desktop)
            .await
            .expect("install");
        assert_eq!(joined, Install::Started("9.9.9".into()));
        assert_eq!(
            app.state::<UpdateGuard>().installing(),
            Some(("9.9.9".to_string(), UpdateOrigin::Remote)),
            "joining doesn't claim or release the guard"
        );
    }

    #[test]
    fn the_restart_marker_is_written_read_and_removed() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(restart_marker_age(dir.path()), None);

        write_restart_marker(dir.path()).expect("write");
        let age = restart_marker_age(dir.path()).expect("marker");
        assert!(age < Duration::from_secs(5));

        std::fs::write(restart_marker(dir.path()), "garbage").expect("overwrite");
        assert_eq!(restart_marker_age(dir.path()), Some(Duration::MAX));

        remove_restart_marker(dir.path());
        assert_eq!(restart_marker_age(dir.path()), None);
    }

    #[test]
    fn install_outcomes_word_the_same_for_every_client() {
        assert_eq!(
            Install::Unavailable.started(),
            Err("no update available".to_string())
        );
        let changed = Install::Changed("2.0.0".into()).started().unwrap_err();
        assert!(changed.contains("2.0.0") && changed.contains("check again"));
    }
}
