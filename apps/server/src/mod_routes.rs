//! Endpoints the `workbench` plugin inside a terminal pane's interactive
//! `claude` uses to run that session as a chat (see `agent::modlink`).
//! Exempt from the bearer check: each request carries the terminal's own
//! token, which only reaches that pane's shell.

use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent::AgentSession;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const TOKEN_HEADER: &str = "x-workbench-mod-token";
/// How long a poll waits for a line before answering empty.
const POLL_WAIT: Duration = Duration::from_secs(20);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRef {
    session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutBody {
    session_id: String,
    lines: Vec<Value>,
}

fn err(status: StatusCode, message: &str) -> ApiError {
    ApiError {
        status,
        message: message.to_string(),
    }
}

fn token(headers: &HeaderMap) -> ApiResult<&str> {
    headers
        .get(TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "missing terminal token"))
}

fn session(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
) -> ApiResult<std::sync::Arc<AgentSession>> {
    state
        .agents
        .mod_session(token(headers)?, id)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such terminal session"))
}

pub async fn hello(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SessionRef>,
) -> ApiResult<StatusCode> {
    let token = token(&headers)?.to_string();
    let agents = state.agents.clone();
    crate::routes::blocking(move || agents.attach_mod(&token, &body.session_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn out(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<OutBody>,
) -> ApiResult<StatusCode> {
    let session = session(&state, &headers, &body.session_id)?;
    let agents = state.agents.clone();
    crate::routes::blocking(move || {
        agents.feed_mod(&session, &body.lines);
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn poll(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SessionRef>,
) -> ApiResult<Json<Vec<Value>>> {
    let session = session(&state, &headers, &q.session_id)?;
    let link = session
        .mod_link()
        .cloned()
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such terminal session"))?;
    Ok(Json(link.take(POLL_WAIT).await))
}

/// Whether a chat view is open on the session, so approvals go to it rather than the TUI.
pub async fn route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SessionRef>,
) -> ApiResult<Json<Value>> {
    let session = session(&state, &headers, &q.session_id)?;
    Ok(Json(json!({ "chat": session.has_viewers() })))
}

pub async fn bye(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SessionRef>,
) -> ApiResult<StatusCode> {
    session(&state, &headers, &body.session_id)?;
    let agents = state.agents.clone();
    crate::routes::blocking(move || Ok(agents.stop(&body.session_id, false))).await?;
    Ok(StatusCode::NO_CONTENT)
}
