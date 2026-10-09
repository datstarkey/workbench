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
use workbench_core::claude_accounts::RateWindow;
use workbench_core::claude_transcript::WaitingSummary;

use crate::agent::AgentSession;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const TOKEN_HEADER: &str = "x-workbench-mod-token";
/// How long a poll waits for a line before answering empty.
const POLL_WAIT: Duration = Duration::from_secs(20);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloBody {
    session_id: String,
    /// The plugin worker's numbering (absent: an older plugin).
    epoch: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutBody {
    session_id: String,
    #[serde(default)]
    lines: Vec<Value>,
    /// The first line's number, the rest following, in the worker's `epoch`
    /// (absent: an older plugin).
    seq: Option<u64>,
    epoch: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PollQuery {
    session_id: String,
    /// The newest line the plugin handled; absent from an older plugin, whose
    /// lines are taken as they're answered.
    ack: Option<u64>,
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
    Json(body): Json<HelloBody>,
) -> ApiResult<Json<Value>> {
    let token = token(&headers)?.to_string();
    // Still attached (the plugin's worker restarted, or an earlier hello's
    // reply was lost): nothing is loaded again.
    let loaded = state.agents.mod_session(&token, &body.session_id).is_none();
    let agents = state.agents.clone();
    let session_id = body.session_id.clone();
    let session = crate::routes::blocking(move || agents.attach_mod(&token, &session_id)).await?;
    let link = session.mod_link();
    if let Some(link) = link {
        link.hello_from(body.epoch.as_deref());
    }
    // Where its `/mod/in` lines stand: 0 for a new link.
    let in_seq = link.map_or(0, |l| l.acked());
    // The plugin can only guess the model list; the CLI's own replaces it
    // when the (cached) probe answers.
    // A stale list is pinned at once; a newer one fetched behind it goes to
    // every session of that account and cwd.
    tokio::spawn(async move {
        let (account, cwd) = (session.claude_account_id(), session.cwd());
        let agents = state.agents.clone();
        // Pinning takes each session's driver lock: off the async workers.
        let repin = {
            let (account, cwd) = (account.clone(), cwd.clone());
            Box::new(move |models: Vec<_>| {
                tokio::task::spawn_blocking(move || agents.pin_models_for(&account, &cwd, &models));
            })
        };
        match state.models.get(account, cwd, repin).await {
            Ok(models) => {
                let _ = tokio::task::spawn_blocking(move || session.pin_models(models)).await;
            }
            Err(e) => tracing::warn!("could not list Claude models: {e:#}"),
        }
    });
    Ok(Json(json!({ "inSeq": in_seq, "loaded": loaded })))
}

pub async fn out(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<OutBody>,
) -> ApiResult<StatusCode> {
    let session = session(&state, &headers, &body.session_id)?;
    note_usage(&state, &session, &body.lines);
    let agents = state.agents.clone();
    crate::routes::blocking(move || {
        agents.feed_mod(&session, &body.lines, body.epoch.as_deref(), body.seq);
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The plan usage windows a `rate_limit_event` carries, as the account's
/// latest reading (`GET /agent/usage` answers from it).
fn note_usage(state: &AppState, session: &AgentSession, lines: &[Value]) {
    for line in lines.iter().filter(|l| l["type"] == "rate_limit_event") {
        let windows = line
            .get("windows")
            .and_then(|w| Vec::<RateWindow>::deserialize(w).ok());
        if let Some(windows) = windows {
            state.usage.note(session.claude_account_id(), &windows);
        }
    }
}

pub async fn poll(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PollQuery>,
) -> ApiResult<Json<Vec<Value>>> {
    let session = session(&state, &headers, &q.session_id)?;
    let link = session
        .mod_link()
        .cloned()
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such terminal session"))?;
    Ok(Json(link.take(POLL_WAIT, q.ack).await))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskBody {
    session_id: String,
    request_id: String,
    /// The `can_use_tool` request, on the first call; later calls keep waiting.
    line: Option<Value>,
    /// Lines queued before it (the tool's card), fed first: waiting on the
    /// plugin's queue would spend the asking hook's budget.
    #[serde(default)]
    lines: Vec<Value>,
    /// The first of `lines`' numbers, as for `/mod/out`.
    seq: Option<u64>,
    epoch: Option<String>,
    /// A chat's to answer even before one has it open: its turn came from chat.
    #[serde(default)]
    hold: bool,
    /// Only the terminal's dialog can answer it (a plan): shown as waiting
    /// there, with no card in chat.
    #[serde(default)]
    terminal: bool,
}

/// How long one `/mod/ask` call waits before answering `pending`.
const ASK_WAIT: Duration = Duration::from_secs(20);

/// An approval the terminal's `claude` asks in chat. Answers `{answer}` once a
/// client answers, `{fallback: true}` when no chat has shown it (the TUI asks
/// instead; the card is withdrawn, but the session list still shows it as
/// waiting, `inTerminal`), or `{pending: true}` to be called again. Once a chat
/// has shown it, or its turn came from chat (`hold`), it waits for a chat's
/// answer even while none is open.
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
    let epoch = body.epoch.clone();
    let feed = |lines: Vec<Value>, seq: Option<u64>| {
        let agents = state.agents.clone();
        let session = session.clone();
        let epoch = epoch.clone();
        crate::routes::blocking(move || {
            agents.feed_mod(&session, &lines, epoch.as_deref(), seq);
            Ok(())
        })
    };
    // Numbered lines fold once however often a retry brings them; an older
    // plugin's only come with the request's first call.
    let numbered = body.seq.is_some();
    if numbered {
        note_usage(&state, &session, &body.lines);
        feed(body.lines.clone(), body.seq).await?;
    }
    if let Some(line) = body.line {
        let tool_use_id = line
            .pointer("/request/tool_use_id")
            .and_then(Value::as_str)
            .map(String::from);
        let new = link.expect_answer(&body.request_id, tool_use_id);
        if new && body.terminal {
            if !numbered {
                note_usage(&state, &session, &body.lines);
                feed(body.lines, None).await?;
            }
            let tool = line.pointer("/request/tool_name").and_then(Value::as_str);
            link.fall_back(
                &body.request_id,
                Some(WaitingSummary {
                    id: body.request_id.clone(),
                    tool: tool.unwrap_or("tool").to_string(),
                    preview: "Waiting in the terminal".to_string(),
                    in_terminal: true,
                }),
            );
            refresh_attention(&session).await?;
        } else if new {
            if body.hold {
                link.shown(&body.request_id, true);
            }
            let mut lines = if numbered { Vec::new() } else { body.lines };
            if !numbered {
                note_usage(&state, &session, &lines);
            }
            lines.push(line);
            feed(lines, None).await?;
        }
    }
    // The terminal asks it (and a retry after a lost reply hears so again).
    if link.fell_back(&body.request_id) {
        return Ok(Json(json!({ "fallback": true })));
    }
    let deadline = tokio::time::Instant::now() + ASK_WAIT;
    loop {
        if let Some(answer) = link
            .wait_answer(&body.request_id, Duration::from_secs(1))
            .await
        {
            return Ok(Json(json!({ "answer": answer })));
        }
        if !link.shown(&body.request_id, session.has_viewers()) {
            // Both read the session under its driver lock: off the async workers.
            let (asking, request_id) = (session.clone(), body.request_id.clone());
            let waiting =
                crate::routes::blocking(move || Ok(asking.waiting_for(&request_id))).await?;
            link.fall_back(&body.request_id, waiting);
            refresh_attention(&session).await?;
            feed(
                vec![
                    json!({"type": "control_cancel_request", "request_id": body.request_id,
                    "workbench_in_terminal": true}),
                ],
                None,
            )
            .await?;
            return Ok(Json(json!({ "fallback": true })));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(Json(json!({ "pending": true })));
        }
    }
}

async fn refresh_attention(session: &std::sync::Arc<AgentSession>) -> ApiResult<()> {
    let session = session.clone();
    crate::routes::blocking(move || {
        session.refresh_attention();
        Ok(())
    })
    .await
}

/// The terminal's `claude` is leaving: its last lines, then the chat detaches.
pub async fn bye(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<OutBody>,
) -> ApiResult<StatusCode> {
    let session = session(&state, &headers, &body.session_id)?;
    note_usage(&state, &session, &body.lines);
    let agents = state.agents.clone();
    crate::routes::blocking(move || {
        agents.feed_mod(&session, &body.lines, body.epoch.as_deref(), body.seq);
        agents.detach(&session);
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
