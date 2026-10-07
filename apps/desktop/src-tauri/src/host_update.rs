//! Lets a paired phone update this app through the embedded server
//! (`/host/update`). The updater verifies each release's signature, so a token
//! holder can only install a genuine Workbench release.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use workbench_core::types::HostUpdateStatus;
use workbench_server::host::{BoxFuture, HostControl};

/// The phone polls `GET /host/update`; one feed check answers it (and its install) this long.
const FEED_TTL: Duration = Duration::from_secs(5 * 60);

pub struct DesktopHost {
    app: AppHandle,
    feed: Mutex<Option<(Instant, Option<Update>)>>,
}

impl DesktopHost {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            feed: Mutex::new(None),
        }
    }

    async fn available(&self) -> anyhow::Result<Option<Update>> {
        if let Some((at, update)) = &*lock(&self.feed) {
            if at.elapsed() < FEED_TTL {
                return Ok(update.clone());
            }
        }
        let update = self.app.updater()?.check().await?;
        *lock(&self.feed) = Some((Instant::now(), update.clone()));
        Ok(update)
    }
}

impl HostControl for DesktopHost {
    fn check(&self) -> BoxFuture<'_, anyhow::Result<HostUpdateStatus>> {
        Box::pin(async move {
            let current = self.app.package_info().version.to_string();
            if self.app.state::<UpdateGuard>().installing().is_some() {
                return Ok(HostUpdateStatus {
                    current,
                    available: None,
                    installing: true,
                });
            }
            // An unreachable feed is "nothing to install", not a failed request.
            let available = match self.available().await {
                Ok(update) => update.map(|u| u.version),
                Err(e) => {
                    log::warn!("update check for a remote client failed: {e:#}");
                    None
                }
            };
            Ok(HostUpdateStatus {
                current,
                available,
                installing: false,
            })
        })
    }

    fn install(&self) -> BoxFuture<'_, anyhow::Result<Option<String>>> {
        Box::pin(async move {
            let guard = self.app.state::<UpdateGuard>();
            if let Some(version) = guard.installing() {
                return Ok(Some(version));
            }
            let Some(update) = self.available().await? else {
                return Ok(None);
            };
            let version = update.version.clone();
            if let Err(running) = guard.begin(&version) {
                return Ok(Some(running));
            }
            let lease = Lease(self.app.clone());
            let _ = self.app.emit("update:remote", &version);
            let app = self.app.clone();
            tauri::async_runtime::spawn(async move {
                let _lease = lease;
                if let Err(e) = install(&app, update).await {
                    log::error!("remote host update failed: {e:#}");
                    let _ = app.emit("update:remote-failed", format!("{e:#}"));
                }
            });
            Ok(Some(version))
        })
    }
}

/// The frontend `UpdaterStore` flow, run from Rust: download while sessions keep
/// running, then end them before installing (on Windows the installer exits the
/// app, and children outliving it keep the old Dock tile or console window).
async fn install(app: &AppHandle, update: Update) -> anyhow::Result<()> {
    log::warn!(
        "installing Workbench {} at a remote client's request",
        update.version
    );
    let bytes = update.download(|_, _| {}, || {}).await?;
    crate::server_control::kill_all_sessions(app.state())
        .await
        .map_err(anyhow::Error::msg)?;
    tauri::async_runtime::spawn_blocking(move || update.install(bytes)).await??;
    app.restart();
}

/// One install at a time, whichever side started it: the desktop's own
/// `UpdaterStore` (`begin_update`/`end_update`) or a phone (`/host/update`).
/// Holds the version being installed.
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

/// Ends a remote install's claim when it fails, so it can be retried.
struct Lease(AppHandle);

impl Drop for Lease {
    fn drop(&mut self) {
        self.0.state::<UpdateGuard>().end();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The desktop's own updater claims the install before downloading.
#[tauri::command]
pub fn begin_update(guard: tauri::State<'_, UpdateGuard>, version: String) -> Result<(), String> {
    guard.begin(&version).map_err(|running| {
        format!("Workbench {running} is already being installed from another device")
    })
}

#[tauri::command]
pub fn end_update(guard: tauri::State<'_, UpdateGuard>) {
    guard.end();
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
