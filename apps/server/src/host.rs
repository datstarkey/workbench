//! Updating the app that hosts an embedded server (`/host/update`). Only the
//! desktop can install itself, so the standalone server has no [`HostControl`]
//! and both routes answer 501.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
pub use futures_util::future::BoxFuture;
use workbench_core::types::{HostUpdateStarted, HostUpdateStatus};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub trait HostControl: Send + Sync {
    fn check(&self) -> BoxFuture<'_, anyhow::Result<HostUpdateStatus>>;
    /// Start installing the available update (or join the install already
    /// running); the host restarts when it's done. Resolves to the version being
    /// installed, `None` when there is nothing to install.
    fn install(&self) -> BoxFuture<'_, anyhow::Result<Option<String>>>;
}

fn host(state: &AppState) -> ApiResult<&dyn HostControl> {
    state.host.as_deref().ok_or_else(|| ApiError {
        status: StatusCode::NOT_IMPLEMENTED,
        message: "this server can't update its host".to_string(),
    })
}

pub async fn update_status(State(state): State<AppState>) -> ApiResult<Json<HostUpdateStatus>> {
    Ok(Json(host(&state)?.check().await?))
}

pub async fn update_install(
    State(state): State<AppState>,
) -> ApiResult<(StatusCode, Json<HostUpdateStarted>)> {
    match host(&state)?.install().await? {
        Some(version) => Ok((StatusCode::ACCEPTED, Json(HostUpdateStarted { version }))),
        None => Err(ApiError {
            status: StatusCode::CONFLICT,
            message: "no update available".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Managers;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tokio::sync::watch;

    struct FakeHost {
        available: Option<String>,
        installed: AtomicBool,
    }

    impl HostControl for FakeHost {
        fn check(&self) -> BoxFuture<'_, anyhow::Result<HostUpdateStatus>> {
            Box::pin(async {
                Ok(HostUpdateStatus {
                    current: "1.0.0".into(),
                    available: self.available.clone(),
                    body: None,
                    installing: self.installed.load(Ordering::SeqCst),
                })
            })
        }

        fn install(&self) -> BoxFuture<'_, anyhow::Result<Option<String>>> {
            Box::pin(async {
                self.installed
                    .store(self.available.is_some(), Ordering::SeqCst);
                Ok(self.available.clone())
            })
        }
    }

    fn state(available: Option<&str>) -> (AppState, Arc<FakeHost>) {
        let fake = Arc::new(FakeHost {
            available: available.map(str::to_string),
            installed: AtomicBool::new(false),
        });
        let managers = Managers {
            host: Some(fake.clone()),
            ..Managers::default()
        };
        (AppState::new(managers, None, watch::channel(false).1), fake)
    }

    #[tokio::test]
    async fn reports_the_host_version_and_installs_the_update() {
        let (state, fake) = state(Some("1.1.0"));
        let Json(status) = update_status(State(state.clone())).await.ok().unwrap();
        assert_eq!(status.current, "1.0.0");
        assert_eq!(status.available.as_deref(), Some("1.1.0"));
        assert!(!status.installing);

        let (code, Json(started)) = update_install(State(state)).await.ok().unwrap();
        assert_eq!(code, StatusCode::ACCEPTED);
        assert_eq!(started.version, "1.1.0");
        assert!(fake.installed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn installing_without_an_update_conflicts() {
        let (state, _) = state(None);
        let err = update_install(State(state)).await.err().unwrap();
        assert_eq!(err.status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn without_a_host_both_routes_are_not_implemented() {
        let state = AppState::new(Managers::default(), None, watch::channel(false).1);
        let check = update_status(State(state.clone())).await.err().unwrap();
        let install = update_install(State(state)).await.err().unwrap();
        assert_eq!(check.status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(install.status, StatusCode::NOT_IMPLEMENTED);
    }
}
