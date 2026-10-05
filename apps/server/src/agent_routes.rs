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
    AgentKind, AgentManager, AgentSession, AgentSummary, CachePolicy, Launch, PromptFile,
    PromptImage, StartAgent, MAX_FILES, MAX_IMAGES,
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
    let terminals = state.terminals.clone();
    let local_port = state.local_port;
    crate::routes::blocking(move || claude_start(&agents, &terminals, local_port, body))
        .await
        .map(Json)
}

/// How long a new terminal's `claude` gets to start and attach through the plugin.
const TERMINAL_START: std::time::Duration = std::time::Duration::from_secs(30);

/// A Claude chat is always an interactive `claude` in a server terminal, run
/// as a chat by the Workbench plugin (`mod_routes`): the terminal and the chat
/// are one process. Blocking: waits for the plugin to attach.
fn claude_start(
    agents: &AgentManager,
    terminals: &crate::terminal::TerminalManager,
    local_port: Option<u16>,
    body: StartBody,
) -> anyhow::Result<Value> {
    claude_validate(&body.session_id, body.permission_mode.as_deref())?;
    if let Some(existing) = agents.get(&body.session_id) {
        return Ok(start_reply(&existing));
    }
    let config_dir =
        workbench_core::claude_accounts::resolve_saved(body.claude_account_id.as_deref())?;
    let resume = crate::agent::claude_history_exists(config_dir.as_deref(), &body.session_id);
    let terminal = crate::terminal::create_from_body(
        terminals,
        agents,
        local_port,
        crate::terminal::CreateTerminalBody {
            project_path: body.project_path,
            worktree_path: body.worktree_path,
            name: None,
            command: None,
            claude_session: Some(crate::terminal::ClaudeSessionLaunch {
                id: body.session_id.clone(),
                resume,
            }),
            cols: 120,
            rows: 40,
            pane_id: body.pane_id,
            hook_socket: body.hook_socket,
            shell: None,
            claude_account_id: body.claude_account_id,
        },
    )?;
    let deadline = std::time::Instant::now() + TERMINAL_START;
    while std::time::Instant::now() < deadline {
        if let Some(session) = agents.get(&body.session_id) {
            return Ok(start_reply(&session));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    terminals.kill(&terminal.id);
    anyhow::bail!(
        "Claude didn't start in its terminal within {}s. Open it as a terminal to see why (a folder trust prompt, a login).",
        TERMINAL_START.as_secs()
    )
}

fn claude_validate(session_id: &str, permission_mode: Option<&str>) -> anyhow::Result<()> {
    if !workbench_core::claude_transcript::is_uuid(session_id) {
        anyhow::bail!("session id must be a UUID");
    }
    if let Some(mode) = permission_mode {
        if !workbench_core::claude_launch::PERMISSION_MODES.contains(&mode) {
            anyhow::bail!("unknown permission mode: {mode}");
        }
    }
    Ok(())
}

fn start_reply(session: &AgentSession) -> Value {
    json!({"sessionId": session.id(), "terminalId": session.summary().terminal_id})
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
    Ok(Json(start_reply(&session)))
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
    let agents = state.agents.clone();
    let reply = crate::routes::blocking(move || Ok(handle(&agents, &session, &body)))
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

#[derive(Debug, Deserialize)]
pub struct StopQuery {
    #[serde(default)]
    end: bool,
}

pub async fn agent_stop(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<StopQuery>,
) -> ApiResult<StatusCode> {
    let agents = state.agents.clone();
    let terminals = state.terminals.clone();
    crate::routes::blocking(move || {
        // Ending a chat ends its terminal `claude` too; a plain stop only detaches.
        let terminal = q
            .end
            .then(|| agents.get(&id).and_then(|s| s.summary().terminal_id))
            .flatten();
        let stopped = agents.stop(&id, q.end);
        if let Some(terminal) = terminal {
            terminals.kill(&terminal);
        }
        Ok(stopped)
    })
    .await?;
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
    let agents = state.agents.clone();
    // A prompt can carry 10 images of up to ~6.7 MB base64 each and 5 PDFs of
    // ~13.4 MB; the default 16 MiB frame limit would drop the socket instead
    // of the prompt.
    Ok(ws
        .max_frame_size(MAX_PROMPT_BYTES)
        .max_message_size(MAX_PROMPT_BYTES)
        .on_upgrade(move |socket| stream(socket, agents, session, revoked)))
}

const MAX_PROMPT_BYTES: usize = 160 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
enum ClientMsg {
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
    /// Refresh the prompt cache now (a hidden keep-alive turn).
    CachePing,
    /// Keep the cache warm until a time and/or compact before it expires.
    CachePolicy {
        policy: CachePolicy,
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
    /// Go back to before the prompt `message_id`: restore the files Claude
    /// changed since (`code`), restart the conversation from there
    /// (`conversation`), or with `dry_run` only report which files would change.
    #[serde(rename_all = "camelCase")]
    Rewind {
        message_id: String,
        #[serde(default)]
        code: bool,
        #[serde(default)]
        conversation: bool,
        #[serde(default)]
        dry_run: bool,
    },
}

/// Apply a client message; `Some` is a reply for that client alone.
fn handle(
    agents: &AgentManager,
    session: &AgentSession,
    text: &str,
) -> anyhow::Result<Option<Value>> {
    let reply = match serde_json::from_str::<ClientMsg>(text)? {
        ClientMsg::Rewind {
            message_id,
            code,
            conversation,
            dry_run,
        } => {
            let result = rewind(agents, session, &message_id, code, conversation, dry_run);
            let (files, error) = match result {
                Ok(files) => (files, None),
                Err(e) => (None, Some(e.to_string())),
            };
            return Ok(Some(json!({
                "t": "rewind", "messageId": message_id, "dryRun": dry_run,
                "files": files, "error": error,
            })));
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
        ClientMsg::CachePing => session.keep_cache_warm(),
        ClientMsg::CachePolicy { policy } => agents.set_cache_policy(session, policy),
    };
    reply.map(|()| None)
}

/// Files first, while the process that tracked them still runs; a file
/// restore that fails leaves the conversation alone.
fn rewind(
    agents: &AgentManager,
    session: &AgentSession,
    message_id: &str,
    code: bool,
    conversation: bool,
    dry_run: bool,
) -> anyhow::Result<Option<Value>> {
    let files = if code {
        Some(session.rewind_files(message_id, dry_run)?)
    } else {
        None
    };
    if dry_run {
        return Ok(files);
    }
    if let Some(f) = &files {
        if f["canRewind"] != true {
            let why = f["error"]
                .as_str()
                .unwrap_or("Claude couldn't restore the files.");
            anyhow::bail!("{why}");
        }
    }
    if conversation {
        agents.rewind_conversation(&session.id(), message_id)?;
    }
    Ok(files)
}

async fn stream(
    mut socket: WebSocket,
    agents: AgentManager,
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
                let ended = frame.contains(r#""t":"exit""#) || frame.contains(r#""t":"replaced""#);
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
                    let agents = agents.clone();
                    let result =
                        tokio::task::spawn_blocking(move || handle(&agents, &worker, &text)).await;
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
