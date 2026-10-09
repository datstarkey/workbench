//! Updating this app: one check, one install and one guard for the desktop's own
//! `UpdaterStore` (`host_update_status`/`host_update_install`) and a paired phone
//! (`/host/update`) alike. The updater verifies each release's signature, so a
//! token holder can only install a genuine Workbench release.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};
use workbench_core::types::{HostUpdateStarted, HostUpdateStatus};
use workbench_server::host::{BoxFuture, HostControl};

/// The phone polls `GET /host/update`; one feed check answers it (and its install) this long.
const FEED_TTL: Duration = Duration::from_secs(5 * 60);

/// `update:progress`: the running install's download, whoever started it.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    downloaded: u64,
    total: Option<u64>,
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

    fn current(&self) -> String {
        self.app.package_info().version.to_string()
    }

    /// `fresh` skips the cached feed answer: a person asked to check.
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

    async fn status(&self, fresh: bool) -> anyhow::Result<HostUpdateStatus> {
        if self.app.state::<UpdateGuard>().installing().is_some() {
            return Ok(HostUpdateStatus {
                current: self.current(),
                available: None,
                body: None,
                installing: true,
            });
        }
        let update = self.available(fresh).await?;
        Ok(HostUpdateStatus {
            current: self.current(),
            available: update.as_ref().map(|u| u.version.clone()),
            body: update.and_then(|u| u.body),
            installing: false,
        })
    }
}

impl<R: Runtime> HostControl for DesktopHost<R> {
    fn check(&self) -> BoxFuture<'_, anyhow::Result<HostUpdateStatus>> {
        Box::pin(async move {
            // For a remote client an unreachable feed is "nothing to install", not a failed request.
            Ok(self.status(false).await.unwrap_or_else(|e| {
                log::warn!("update check for a remote client failed: {e:#}");
                HostUpdateStatus {
                    current: self.current(),
                    available: None,
                    body: None,
                    installing: false,
                }
            }))
        })
    }

    fn install(&self) -> BoxFuture<'_, anyhow::Result<Option<String>>> {
        Box::pin(async move {
            let guard = self.app.state::<UpdateGuard>();
            if let Some(version) = guard.installing() {
                return Ok(Some(version));
            }
            let Some(update) = self.available(false).await? else {
                return Ok(None);
            };
            let version = update.version.clone();
            if let Err(running) = guard.begin(&version) {
                return Ok(Some(running));
            }
            let lease = Lease(self.app.clone());
            let _ = self.app.emit("update:installing", &version);
            let app = self.app.clone();
            tauri::async_runtime::spawn(async move {
                let _lease = lease;
                if let Err(e) = install(&app, update).await {
                    log::error!("update install failed: {e:#}");
                    let _ = app.emit("update:failed", format!("{e:#}"));
                }
            });
            Ok(Some(version))
        })
    }
}

/// Download while sessions keep running, then end them before installing (on
/// Windows the installer exits the app, and children outliving it keep the old
/// Dock tile or console window).
async fn install<R: Runtime>(app: &AppHandle<R>, update: Update) -> anyhow::Result<()> {
    log::warn!("installing Workbench {}", update.version);
    let mut downloaded = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = app.emit("update:progress", UpdateProgress { downloaded, total });
            },
            || {},
        )
        .await?;
    crate::server_control::kill_all_sessions(&app.state::<crate::server_control::ServerControl>())
        .await;
    tauri::async_runtime::spawn_blocking(move || update.install(bytes)).await??;
    app.restart();
}

/// One install at a time, whichever side started it. Holds the version being installed.
#[derive(Default)]
pub struct UpdateGuard(Mutex<Option<String>>);

impl UpdateGuard {
    /// Claim the install for `version`; `Err` names the version already installing.
    fn begin(&self, version: &str) -> Result<(), String> {
        let mut slot = lock(&self.0);
        match &*slot {
            Some(running) => Err(running.clone()),
            None => {
                *slot = Some(version.to_string());
                Ok(())
            }
        }
    }

    fn end(&self) {
        *lock(&self.0) = None;
    }

    fn installing(&self) -> Option<String> {
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

/// The desktop's own check: unlike a phone's, a feed failure is an error to show.
#[tauri::command]
pub async fn host_update_status(
    host: tauri::State<'_, Arc<DesktopHost>>,
) -> Result<HostUpdateStatus, String> {
    host.status(true).await.map_err(|e| format!("{e:#}"))
}

/// Starts the install, or joins the one running; progress and failure arrive as `update:*` events.
#[tauri::command]
pub async fn host_update_install(
    host: tauri::State<'_, Arc<DesktopHost>>,
) -> Result<HostUpdateStarted, String> {
    started(host.install().await)
}

fn started(installing: anyhow::Result<Option<String>>) -> Result<HostUpdateStarted, String> {
    match installing {
        Ok(Some(version)) => Ok(HostUpdateStarted { version }),
        Ok(None) => Err("no update available".to_string()),
        Err(e) => Err(format!("{e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[test]
    fn only_one_install_runs_until_it_ends() {
        let guard = UpdateGuard::default();
        guard.begin("1.1.0").expect("first install starts");
        assert_eq!(guard.installing().as_deref(), Some("1.1.0"));
        assert_eq!(guard.begin("1.1.0"), Err("1.1.0".to_string()));

        guard.end();
        assert_eq!(guard.installing(), None);
        assert!(
            guard.begin("1.2.0").is_ok(),
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
            .begin("9.9.9")
            .expect("a phone's install");

        let status = host.status(true).await.expect("status");
        assert!(status.installing);
        assert_eq!(status.available, None);
        assert!(host.check().await.expect("remote check").installing);

        assert_eq!(
            started(host.install().await),
            Ok(HostUpdateStarted {
                version: "9.9.9".into()
            })
        );
        assert_eq!(
            app.state::<UpdateGuard>().installing().as_deref(),
            Some("9.9.9"),
            "joining doesn't claim or release the guard"
        );
    }

    #[test]
    fn nothing_to_install_is_an_error_for_the_desktop() {
        assert_eq!(started(Ok(None)), Err("no update available".to_string()));
        assert_eq!(
            started(Err(anyhow::anyhow!("offline"))),
            Err("offline".to_string())
        );
    }
}
