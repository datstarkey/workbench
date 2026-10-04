//! HTTP + WebSocket surface for chat sessions (`:kind` is `claude` or `codex`):
//! - `GET /agent` lists every live session ([`AgentSummary`], with `agent`),
//!   newest change first; `GET /agent/:kind` only that kind's (older phone
//!   builds read `/agent/claude`).
//! - `POST /agent/claude` starts (or returns) the session for a Claude session
//!   id; `POST /agent/codex` starts a new Codex thread (no `sessionId`) or
//!   resumes one, and answers once codex has its id. With `attachOnly` either
//!   only returns a running session (404 otherwise).
//! - `DELETE /agent/:kind/:id` stops it; `DELETE /agent/:kind?paneId=` stops
//!   whatever a closed pane owned.
//! - `WS /agent/:kind/:id/ws` streams `snapshot` then `update`/`exit` frames and
//!   takes `prompt` / `approve` / `elicit` / `interrupt` / `mode` messages. Any number of
//!   clients may attach; the first answer to an approval wins.
//! - `POST /agent/:kind/:id/message` applies one of those messages without a
//!   socket (an approval from the phone's home screen): 200 with the reply
//!   frame when there is one, else 204.
//!
//! Ids are global, so the stop/message/WS routes of either kind reach any session.
//! - `GET /agent/usage?claudeAccountId=[&fresh=true]` is the account's plan
//!   usage (`claude -p /usage`), cached by [`crate::usage::UsageCache`].
//! - `GET /agent/files?projectPath=[&worktreePath=]` lists a chat cwd's files
//!   (git-tracked and untracked, not ignored) for the composer's `@` mentions.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{broadcast, watch};
use workbench_core::claude_accounts::{self, UsageLimit};
use workbench_core::claude_transcript::{ApprovalDecision, ElicitationAction};

use crate::agent::{
    AgentKind, AgentSession, AgentSummary, Launch, PromptFile, PromptImage, StartAgent, MAX_FILES,
    MAX_IMAGES,
};
use crate::error::{ApiError, ApiResult};
use crate::spawn::RemoteControlManager;
use crate::state::{wait_revoked, AppState};
use crate::terminal::WsAuthQuery;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartBody {
    pub project_path: String,
    pub worktree_path: Option<String>,
    pub session_id: String,
    pub permission_mode: Option<String>,
    pub pane_id: Option<String>,
    pub hook_socket: Option<String>,
    /// Claude account to run under; an id, never a path (see `claude_accounts`).
    pub claude_account_id: Option<String>,
    /// Join the running session only, never spawn one (a chat another device owns).
    #[serde(default)]
    pub attach_only: bool,
}

pub async fn agent_start(
    State(state): State<AppState>,
    Json(body): Json<StartBody>,
) -> ApiResult<Json<Value>> {
    if body.attach_only {
        // Spawns nothing, so neither the sandbox nor the cwd checks apply.
        return attach_only(&state, &body.session_id);
    }
    let agents = state.agents.clone();
    crate::routes::blocking(move || {
        // Sandboxed Claude runs through srt's launcher; chat mode spawns the CLI
        // directly, so refuse rather than silently run it unsandboxed.
        if workbench_core::config::load_workbench_settings()?.sandbox_runtime_enabled {
            anyhow::bail!(
                "Chat mode doesn't run inside the sandbox runtime yet. Use the terminal, or turn the sandbox off in Settings."
            );
        }
        let cwd = resolve_cwd(&body.project_path, body.worktree_path.as_deref())?;
        let config_dir =
            workbench_core::claude_accounts::resolve_saved(body.claude_account_id.as_deref())?;
        let session = agents.start(StartAgent {
            cwd,
            project_path: body.project_path,
            worktree_path: body.worktree_path,
            pane_id: body.pane_id,
            hook_socket: body.hook_socket,
            claude_account_id: body.claude_account_id,
            launch: Launch::Claude {
                session_id: body.session_id,
                permission_mode: body.permission_mode,
                config_dir,
            },
        })?;
        Ok(json!({"sessionId": session.id()}))
    })
    .await
    .map(Json)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexStartBody {
    pub project_path: String,
    pub worktree_path: Option<String>,
    /// The thread to resume; absent starts a new one.
    pub session_id: Option<String>,
    /// `read-only` | `auto` | `full-access`; absent leaves `~/.codex/config.toml` in charge.
    pub codex_mode: Option<String>,
    #[serde(flatten)]
    pub options: workbench_core::codex_controls::LaunchOptions,
    pub pane_id: Option<String>,
    pub hook_socket: Option<String>,
    #[serde(default)]
    pub attach_only: bool,
}

/// Codex never runs under the sandbox runtime (srt only wraps Claude), so
/// unlike Claude chat it isn't refused while that's on.
pub async fn codex_start(
    State(state): State<AppState>,
    Json(body): Json<CodexStartBody>,
) -> ApiResult<Json<Value>> {
    if body.attach_only {
        let id = body
            .session_id
            .ok_or_else(|| ApiError::bad_request("attachOnly needs a sessionId"))?;
        return attach_only(&state, &id);
    }
    let agents = state.agents.clone();
    crate::routes::blocking(move || {
        let cwd = resolve_cwd(&body.project_path, body.worktree_path.as_deref())?;
        let session = agents.start(StartAgent {
            cwd,
            project_path: body.project_path,
            worktree_path: body.worktree_path,
            pane_id: body.pane_id,
            hook_socket: body.hook_socket,
            claude_account_id: None,
            launch: Launch::Codex {
                thread_id: body.session_id,
                mode: body.codex_mode,
                options: body.options,
            },
        })?;
        Ok(json!({"sessionId": session.id()}))
    })
    .await
    .map(Json)
}

/// The running session for `id`, or 404: a chat another device owns.
fn attach_only(state: &AppState, id: &str) -> ApiResult<Json<Value>> {
    let session = state.agents.get(id).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: "This chat ended on the other device.".into(),
    })?;
    Ok(Json(json!({"sessionId": session.id()})))
}

/// The cwd a chat may run in: a registered project or one of its worktrees.
fn resolve_cwd(project_path: &str, worktree_path: Option<&str>) -> anyhow::Result<String> {
    let registered: Vec<String> = workbench_core::config::load_projects()?
        .into_iter()
        .map(|p| p.path)
        .collect();
    RemoteControlManager::resolve_cwd(project_path, worktree_path, &registered)
}

pub async fn agent_list(State(state): State<AppState>) -> Json<Vec<AgentSummary>> {
    Json(state.agents.summaries(None))
}

pub async fn claude_list(State(state): State<AppState>) -> Json<Vec<AgentSummary>> {
    Json(state.agents.summaries(Some(AgentKind::Claude)))
}

pub async fn codex_list(State(state): State<AppState>) -> Json<Vec<AgentSummary>> {
    Json(state.agents.summaries(Some(AgentKind::Codex)))
}

pub async fn agent_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: String,
) -> Result<Response, ApiError> {
    let session = find(&state, &id)?;
    let reply = crate::routes::blocking(move || Ok(handle(&session, &body)))
        .await?
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(match reply {
        Some(reply) => Json(reply).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    })
}

fn find(state: &AppState, id: &str) -> Result<Arc<AgentSession>, ApiError> {
    state.agents.get(id).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("no chat session {id}"),
    })
}

pub async fn agent_stop(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let agents = state.agents.clone();
    crate::routes::blocking(move || Ok(agents.stop(&id))).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneQuery {
    pane_id: String,
}

pub async fn agent_stop_pane(
    State(state): State<AppState>,
    Query(q): Query<PaneQuery>,
) -> ApiResult<StatusCode> {
    let agents = state.agents.clone();
    crate::routes::blocking(move || Ok(agents.stop_pane(&q.pane_id))).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageQuery {
    claude_account_id: Option<String>,
    /// Skip all but a very recent cached result, e.g. right after a turn.
    #[serde(default)]
    fresh: bool,
}

pub async fn agent_usage(
    State(state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> ApiResult<Json<Vec<UsageLimit>>> {
    let settings = crate::routes::blocking(workbench_core::config::load_workbench_settings).await?;
    // An unknown id is refused, never answered with the default login's usage.
    claude_accounts::resolve(&settings, q.claude_account_id.as_deref())
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(state.usage.get(q.claude_account_id, q.fresh).await?))
}

pub async fn agent_attach(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(auth): Query<WsAuthQuery>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    crate::auth::authorize_ws(&headers, auth.token.as_deref(), &state)?;
    let session = find(&state, &id)?;
    let revoked = state.revoked.clone();
    // A prompt can carry 10 images of up to ~6.7 MB base64 each and 5 PDFs of
    // ~13.4 MB; the default 16 MiB frame limit would drop the socket instead
    // of the prompt.
    Ok(ws
        .max_frame_size(MAX_PROMPT_BYTES)
        .max_message_size(MAX_PROMPT_BYTES)
        .on_upgrade(move |socket| stream(socket, session, revoked)))
}

const MAX_PROMPT_BYTES: usize = 160 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
enum ClientMsg {
    Artifacts {
        id: String,
    },
    #[serde(rename_all = "camelCase")]
    Codex {
        request_id: String,
        action: workbench_core::codex_controls::Action,
        #[serde(default)]
        params: Value,
    },
    Prompt {
        #[serde(default)]
        text: String,
        #[serde(default)]
        images: Vec<PromptImage>,
        #[serde(default)]
        files: Vec<PromptFile>,
    },
    #[serde(rename_all = "camelCase")]
    Approve {
        request_id: String,
        decision: ApprovalDecision,
        /// `AskUserQuestion` answers: question text → chosen label or own words.
        #[serde(default)]
        answers: Option<serde_json::Map<String, Value>>,
    },
    /// Answer an MCP elicitation; `content` is the filled-in form.
    #[serde(rename_all = "camelCase")]
    Elicit {
        request_id: String,
        action: ElicitationAction,
        #[serde(default)]
        content: Option<serde_json::Map<String, Value>>,
    },
    Interrupt,
    Mode {
        mode: String,
    },
    Model {
        model: String,
    },
    Effort {
        effort: String,
    },
    /// Fetch the whole output of a tool shown as a preview.
    #[serde(rename_all = "camelCase")]
    Output {
        tool_id: String,
    },
    /// Fetch the end of a background task's live output.
    #[serde(rename_all = "camelCase")]
    TaskOutput {
        task_id: String,
    },
}

/// Apply a client message; `Some` is a reply for that client alone.
fn handle(session: &AgentSession, text: &str) -> anyhow::Result<Option<Value>> {
    let reply = match serde_json::from_str::<ClientMsg>(text)? {
        ClientMsg::Artifacts { id } => {
            return Ok(Some(
                json!({"t":"artifacts","id":id,"content":session.artifacts(&id).unwrap_or_default()}),
            ))
        }
        ClientMsg::Codex {
            request_id,
            action,
            params,
        } => {
            if request_id.len() > 128
                || params.to_string().len()
                    > if matches!(action, workbench_core::codex_controls::Action::QueueAdd) {
                        32 * 1024 * 1024
                    } else {
                        2 * 1024 * 1024
                    }
            {
                return Ok(Some(
                    json!({"t":"codexResult","requestId":request_id,"error":"Codex control exceeds its size limit"}),
                ));
            }
            if let Err(e) = session.codex_action(&request_id, action, &params) {
                return Ok(Some(
                    json!({"t":"codexResult", "requestId":request_id,"error":e.to_string()}),
                ));
            }
            Ok(())
        }
        ClientMsg::TaskOutput { task_id } => {
            let (text, bytes) = session.task_output(&task_id).unzip();
            return Ok(Some(
                json!({"t": "taskOutput", "taskId": task_id, "text": text, "bytes": bytes}),
            ));
        }
        ClientMsg::Output { tool_id } => {
            let text = session.full_output(&tool_id);
            return Ok(Some(
                json!({"t": "output", "toolId": tool_id, "text": text}),
            ));
        }
        ClientMsg::Prompt {
            text,
            images,
            files,
        } if text.trim().is_empty() && images.is_empty() && files.is_empty() => Ok(()),
        ClientMsg::Prompt {
            text,
            images,
            files,
        } => {
            if images.len() > MAX_IMAGES {
                anyhow::bail!("attach at most {MAX_IMAGES} images per message");
            }
            if files.len() > MAX_FILES {
                anyhow::bail!("attach at most {MAX_FILES} files per message");
            }
            images.iter().try_for_each(PromptImage::validate)?;
            files.iter().try_for_each(PromptFile::validate)?;
            session.prompt(&text, &images, &files)
        }
        ClientMsg::Approve {
            request_id,
            decision,
            answers,
        } => session.approve(&request_id, decision, answers.as_ref()),
        ClientMsg::Elicit {
            request_id,
            action,
            content,
        } => session.elicit(&request_id, action, content.as_ref()),
        ClientMsg::Interrupt => session.interrupt(),
        ClientMsg::Mode { mode } => session.set_mode(&mode),
        ClientMsg::Model { model } => session.set_model(&model),
        ClientMsg::Effort { effort } => session.set_effort(&effort),
    };
    reply.map(|()| None)
}

async fn stream(
    mut socket: WebSocket,
    session: Arc<AgentSession>,
    mut revoked: watch::Receiver<bool>,
) {
    let (snapshot, mut rx) = session.subscribe();
    let already_exited = session.has_exited();
    if socket.send(Message::Text(snapshot)).await.is_err() || already_exited {
        // A finished session has nothing more to say; holding the socket open
        // would pin it (and its transcript) in memory.
        let _ = socket.close().await;
        return;
    }
    loop {
        tokio::select! {
            frame = rx.recv() => {
                let frame = match frame {
                    Ok(frame) => frame,
                    // Fell behind: start over from a fresh snapshot.
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let (snapshot, fresh) = session.subscribe();
                        rx = fresh;
                        snapshot
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                let ended = frame.contains(r#""t":"exit""#);
                if socket.send(Message::Text(frame)).await.is_err() {
                    return;
                }
                if ended {
                    let _ = socket.close().await;
                    return;
                }
            }
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(text))) => {
                    // Pipe writes (a prompt full of images) and the task-output
                    // directory walk block, so keep them off the async workers.
                    let worker = session.clone();
                    let result = tokio::task::spawn_blocking(move || handle(&worker, &text)).await;
                    let frame = match result {
                        Ok(Ok(reply)) => reply,
                        Ok(Err(e)) => Some(json!({"t": "error", "message": e.to_string()})),
                        Err(e) => Some(json!({"t": "error", "message": e.to_string()})),
                    };
                    if let Some(frame) = frame {
                        if socket.send(Message::Text(frame.to_string())).await.is_err() {
                            return;
                        }
                    }
                }
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                Some(Ok(_)) => {}
            },
            _ = wait_revoked(&mut revoked) => {
                let _ = socket.send(Message::Text(r#"{"t":"revoked"}"#.to_string())).await;
                let _ = socket.close().await;
                return;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesQuery {
    project_path: String,
    worktree_path: Option<String>,
}

/// Same cwd allowlist as starting a chat: a registered project or its worktree.
pub async fn agent_files(Query(q): Query<FilesQuery>) -> ApiResult<Json<Vec<String>>> {
    crate::routes::blocking(move || {
        let cwd = resolve_cwd(&q.project_path, q.worktree_path.as_deref())?;
        workbench_core::project_files::list(&cwd, workbench_core::project_files::MAX_LISTED)
    })
    .await
    .map(Json)
}
