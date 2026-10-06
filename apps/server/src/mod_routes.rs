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
    let session =
        crate::routes::blocking(move || agents.attach_mod(&token, &body.session_id)).await?;
    // The plugin can only guess the model list; the CLI's own replaces it
    // when the (cached) probe answers.
    tokio::spawn(async move {
        match state
            .models
            .get(session.claude_account_id(), session.cwd())
            .await
        {
            Ok(models) => session.pin_models(models),
            Err(e) => tracing::warn!("could not list Claude models: {e:#}"),
        }
    });
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskBody {
    session_id: String,
    request_id: String,
    /// The `can_use_tool` request, on the first call; later calls keep waiting.
    line: Option<Value>,
}

/// How long one `/mod/ask` call waits before answering `pending`.
const ASK_WAIT: Duration = Duration::from_secs(20);

/// An approval the terminal's `claude` asks in chat. Answers `{answer}` once a
/// client answers, `{fallback: true}` when no chat is open (the TUI asks
/// instead; the card is withdrawn, but the session list still shows it as
/// waiting, `inTerminal`), or `{pending: true}` to be called again.
/// A held request is how the plugin waits without spending its hook budget.
pub async fn ask(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AskBody>,
) -> ApiResult<Json<Value>> {
    let session = session(&state, &headers, &body.session_id)?;
    let link = session
        .mod_link()
        .cloned()
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such terminal session"))?;
    let feed = |line: Value| {
        let agents = state.agents.clone();
        let session = session.clone();
        crate::routes::blocking(move || {
            agents.feed_mod(&session, &[line]);
            Ok(())
        })
    };
    if let Some(line) = body.line {
        let tool_use_id = line
            .pointer("/request/tool_use_id")
            .and_then(Value::as_str)
            .map(String::from);
        link.expect_answer(&body.request_id, tool_use_id);
        feed(line).await?;
    }
    let deadline = tokio::time::Instant::now() + ASK_WAIT;
    loop {
        if let Some(answer) = link
            .wait_answer(&body.request_id, Duration::from_secs(1))
            .await
        {
            return Ok(Json(json!({ "answer": answer })));
        }
        if !session.has_viewers() {
            link.fall_back(&body.request_id, session.waiting_for(&body.request_id));
            feed(json!({"type": "control_cancel_request", "request_id": body.request_id})).await?;
            return Ok(Json(json!({ "fallback": true })));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(Json(json!({ "pending": true })));
        }
    }
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
