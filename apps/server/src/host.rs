//! Updating the app that hosts an embedded server (`/host/update`). Only the
//! desktop can install itself, so the standalone server has no [`HostControl`]
//! and both routes answer 501. The desktop's own UI drives the same
//! [`HostControl`] through IPC, with the same contract.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
pub use futures_util::future::BoxFuture;
use serde::Deserialize;
use workbench_core::types::{HostUpdateInstall, HostUpdateStarted, HostUpdateStatus, UpdateOrigin};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub trait HostControl: Send + Sync {
    /// `fresh` asks the update feed again instead of reusing a recent answer.
    fn check(&self, fresh: bool) -> BoxFuture<'_, anyhow::Result<HostUpdateStatus>>;
    /// Start installing `version` (the one the client reviewed; `None` takes
    /// whatever is available), or join the install already running. The host
    /// restarts when it's done.
    fn install(
        &self,
        version: Option<String>,
        origin: UpdateOrigin,
    ) -> BoxFuture<'_, anyhow::Result<Install>>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Install {
    /// Installing this version (a joined install may be another than the one asked for).
    Started(String),
    Unavailable,
    /// The feed now offers this version instead of the reviewed one.
    Changed(String),
}

impl Install {
    /// The one wording of an install's outcome, for HTTP and IPC alike.
    pub fn started(self) -> Result<HostUpdateStarted, String> {
        match self {
            Install::Started(version) => Ok(HostUpdateStarted { version }),
            Install::Unavailable => Err("no update available".to_string()),
            Install::Changed(version) => Err(format!(
                "Workbench {version} is available now, not the version you reviewed; check again to review it"
            )),
        }
    }
}

fn host(state: &AppState) -> ApiResult<&dyn HostControl> {
    state.host.as_deref().ok_or_else(|| ApiError {
        status: StatusCode::NOT_IMPLEMENTED,
        message: "this server can't update its host".to_string(),
    })
}

#[derive(Deserialize, Default)]
pub struct StatusQuery {
    #[serde(default)]
    fresh: bool,
}

pub async fn update_status(
    State(state): State<AppState>,
    Query(q): Query<StatusQuery>,
) -> ApiResult<Json<HostUpdateStatus>> {
    Ok(Json(host(&state)?.check(q.fresh).await?))
}

pub async fn update_install(
    State(state): State<AppState>,
    body: Option<Json<HostUpdateInstall>>,
) -> ApiResult<(StatusCode, Json<HostUpdateStarted>)> {
    let version = body.and_then(|Json(b)| b.version);
    let outcome = host(&state)?.install(version, UpdateOrigin::Remote).await?;
    outcome
        .started()
        .map(|started| (StatusCode::ACCEPTED, Json(started)))
        .map_err(|message| ApiError {
            status: StatusCode::CONFLICT,
            message,
        })
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
        fresh: AtomicBool,
    }

    impl HostControl for FakeHost {
        fn check(&self, fresh: bool) -> BoxFuture<'_, anyhow::Result<HostUpdateStatus>> {
            self.fresh.store(fresh, Ordering::SeqCst);
            Box::pin(async {
                Ok(HostUpdateStatus {
                    current: "1.0.0".into(),
                    available: self.available.clone(),
                    body: None,
                    installing: self.installed.load(Ordering::SeqCst),
                    started_by: None,
                })
            })
        }

        fn install(
            &self,
            version: Option<String>,
            origin: UpdateOrigin,
        ) -> BoxFuture<'_, anyhow::Result<Install>> {
            assert_eq!(origin, UpdateOrigin::Remote);
            Box::pin(async move {
                Ok(match (&self.available, version) {
                    (None, _) => Install::Unavailable,
                    (Some(a), Some(v)) if *a != v => Install::Changed(a.clone()),
                    (Some(a), _) => {
                        self.installed.store(true, Ordering::SeqCst);
                        Install::Started(a.clone())
                    }
                })
            })
        }
    }

    fn state(available: Option<&str>) -> (AppState, Arc<FakeHost>) {
        let fake = Arc::new(FakeHost {
            available: available.map(str::to_string),
            installed: AtomicBool::new(false),
            fresh: AtomicBool::new(false),
        });
        let managers = Managers {
            host: Some(fake.clone()),
            ..Managers::default()
        };
        (AppState::new(managers, None, watch::channel(false).1), fake)
    }

    fn reviewed(version: &str) -> Option<Json<HostUpdateInstall>> {
        Some(Json(HostUpdateInstall {
            version: Some(version.to_string()),
        }))
    }

    #[tokio::test]
    async fn reports_the_host_version_and_installs_the_update() {
        let (state, fake) = state(Some("1.1.0"));
        let Json(status) = update_status(State(state.clone()), Query(StatusQuery::default()))
            .await
            .ok()
            .unwrap();
        assert_eq!(status.current, "1.0.0");
        assert_eq!(status.available.as_deref(), Some("1.1.0"));
        assert!(!status.installing);
        assert!(!fake.fresh.load(Ordering::SeqCst));

        assert!(
            update_status(State(state.clone()), Query(StatusQuery { fresh: true }))
                .await
                .is_ok()
        );
        assert!(fake.fresh.load(Ordering::SeqCst));

        let (code, Json(started)) = update_install(State(state), reviewed("1.1.0"))
            .await
            .ok()
            .unwrap();
        assert_eq!(code, StatusCode::ACCEPTED);
        assert_eq!(started.version, "1.1.0");
        assert!(fake.installed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn an_older_client_without_a_version_installs_what_is_available() {
        let (state, _) = state(Some("1.1.0"));
        let (code, _) = update_install(State(state), None).await.ok().unwrap();
        assert_eq!(code, StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn installing_without_an_update_or_another_version_conflicts() {
        let (none, _) = state(None);
        let err = update_install(State(none), None).await.err().unwrap();
        assert_eq!(err.status, StatusCode::CONFLICT);

        let (newer, fake) = state(Some("1.2.0"));
        let err = update_install(State(newer), reviewed("1.1.0"))
            .await
            .err()
            .unwrap();
        assert_eq!(err.status, StatusCode::CONFLICT);
        assert!(err.message.contains("1.2.0"), "{}", err.message);
        assert!(!fake.installed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn without_a_host_both_routes_are_not_implemented() {
        let state = AppState::new(Managers::default(), None, watch::channel(false).1);
        let check = update_status(State(state.clone()), Query(StatusQuery::default()))
            .await
            .err()
            .unwrap();
        let install = update_install(State(state), None).await.err().unwrap();
        assert_eq!(check.status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(install.status, StatusCode::NOT_IMPLEMENTED);
    }
}
