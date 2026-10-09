//! - `POST /workspace/commands`: one `Command` (JSON, `{"type": …}`), answered
//!   `{rev, workspaceId?, tabId?, paneId?}` once a snapshot at that `rev`
//!   includes it, or `{rev, error}` (400) when the model refused it.
//! - `GET /events/workspace`: Server-Sent Events. `snapshot` frames carry the
//!   whole model with each pane's runtime state, `{rev, workspaces}`: one on
//!   connect, then one per new `rev` (at most every 150ms, never a repeat).
//!   `ping` after 15s of quiet. EventSource can't send headers, so the token
//!   may come as `?token=`. The stream ends when its listener is revoked.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::json;
use workbench_core::workspace::{Command, Renderer};

use crate::error::ApiError;
use crate::state::{wait_revoked, AppState};

pub const EVENTS_PATH: &str = "/events/workspace";
const HEARTBEAT: Duration = Duration::from_secs(15);
/// How long a command waits for the snapshot that shows it.
const PUBLISH_WAIT: Duration = Duration::from_secs(2);

pub async fn command(State(state): State<AppState>, Json(cmd): Json<Command>) -> Response {
    let service = state.workspace.clone();
    let rev = || service.subscribe().borrow().rev;
    // Fold-ins report what a process did: only the server sees that.
    if matches!(
        cmd,
        Command::SessionAttached { .. }
            | Command::SessionRekeyed { .. }
            | Command::AccountMoved { .. }
    ) {
        return refused(rev(), "Only the server reports what a session did".into());
    }
    // A native view exists only in the desktop app, for its own webview.
    let native = matches!(
        cmd,
        Command::OpenWorkspace {
            renderer: Renderer::Native,
            ..
        }
    );
    if native && !(state.native_views && crate::auth::serves_mod(&state)) {
        return refused(
            rev(),
            "Native terminals open only in the desktop app on this machine".into(),
        );
    }
    let applied = {
        let service = service.clone();
        tokio::task::spawn_blocking(move || service.command(cmd)).await
    };
    let applied = match applied {
        Ok(Ok(applied)) => applied,
        Ok(Err(e)) => return refused(rev(), format!("{e:#}")),
        Err(e) => return refused(rev(), e.to_string()),
    };
    let mut published = service.subscribe();
    let shown =
        tokio::time::timeout(PUBLISH_WAIT, published.wait_for(|p| p.gen >= applied.gen)).await;
    let rev = match shown {
        Ok(Ok(p)) => p.rev,
        _ => rev(),
    };
    Json(json!({
        "rev": rev,
        "workspaceId": applied.workspace_id,
        "tabId": applied.tab_id,
        "paneId": applied.pane_id,
    }))
    .into_response()
}

fn refused(rev: u64, error: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "rev": rev, "error": error })),
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct EventsQuery {
    token: Option<String>,
}

pub async fn events(
    State(state): State<AppState>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let presented = query
        .token
        .as_deref()
        .or_else(|| crate::auth::bearer(&headers));
    if !crate::auth::token_ok(&state, presented) || *state.revoked.borrow() {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "unauthorized".to_string(),
        });
    }
    let feed = (
        state.workspace.subscribe(),
        state.revoked.clone(),
        None::<u64>,
    );
    let stream =
        futures_util::stream::unfold(feed, |(mut published, mut revoked, sent)| async move {
            loop {
                let snapshot = published.borrow_and_update().clone();
                if !snapshot.json.is_empty() && sent.is_none_or(|rev| snapshot.rev > rev) {
                    let event = Event::default().event("snapshot").data(&snapshot.json);
                    return Some((Ok(event), (published, revoked, Some(snapshot.rev))));
                }
                tokio::select! {
                    _ = wait_revoked(&mut revoked) => return None,
                    changed = published.changed() => if changed.is_err() { return None },
                }
            }
        });
    let ping = Event::default().event("ping").data("{}");
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(HEARTBEAT).event(ping)))
}
