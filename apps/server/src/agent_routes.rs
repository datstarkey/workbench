//! HTTP + WebSocket surface for chat sessions (`:kind` is `claude` or `codex`).
//! Sessions are started and ended only by the workspace service (a pane's
//! process); these routes list them and carry their IO.
//! - `GET /agent` lists every live session ([`AgentSummary`], with `agent`),
//!   newest change first.
//! - `GET /agent/attention?cursor=` long-polls shared notification events.
//! - `WS /agent/:kind/:id/ws` (any of the session's ids; one that isn't running
//!   is closed at once with code 4404) streams `snapshot` then `update`/`exit` frames and
//!   takes `prompt` / `approve` / `elicit` / `interrupt` / `mode` messages. Any number of
//!   clients may attach; the first answer to an approval wins.
//! - `POST /agent/:kind/:id/message` applies one of those messages without a
//!   socket (an approval from the phone's home screen): 200 with the reply
//!   frame when there is one, else 204.
//! - `GET /agent/claude/:id/tasks/:taskId/transcript` is a subagent's own
//!   conversation as chat items, from the CLI's `subagents/` transcript.
//!
//! Ids are global, so the message/WS routes of either kind reach any session.
//! - `GET /agent/usage?claudeAccountId=[&fresh=true]` is the account's plan
//!   usage (a live session's reading, else `claude -p /usage`), from
//!   [`crate::usage::UsageCache`].
//! - `GET /agent/files?projectPath=[&worktreePath=]` lists a chat cwd's files
//!   (git-tracked and untracked, not ignored) for the composer's `@` mentions.

use std::sync::Arc;
use std::time::Duration;

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
    AgentKind, AgentSession, AgentSummary, CachePolicy, PromptFile, PromptImage, MAX_FILES,
    MAX_IMAGES,
};
use crate::cwd::resolve_cwd;
use crate::error::{ApiError, ApiResult};
use crate::state::{wait_revoked, ws_close, ws_send, AppState};
use crate::terminal::WsAuthQuery;

/// Off the async workers: a summary takes each session's driver lock, which a
/// busy session may hold for a while (a snapshot, a long line).
pub async fn agent_list(State(state): State<AppState>) -> ApiResult<Json<Vec<AgentSummary>>> {
    let agents = state.agents.clone();
    crate::routes::blocking(move || Ok(agents.summaries()))
        .await
        .map(Json)
}

#[derive(Deserialize)]
pub struct AttentionQuery {
    cursor: Option<String>,
}

/// The same buffered attention events the desktop emits, with a cursor so a
/// sleeping phone can catch up. A new connection only seeds its position.
pub async fn agent_attention(
    State(state): State<AppState>,
    Query(query): Query<AttentionQuery>,
) -> ApiResult<Json<crate::attention_feed::AttentionBatch>> {
    let mut revoked = state.revoked.clone();
    let until = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut changes = state.agents.attention.subscribe();
    loop {
        if *revoked.borrow() {
            return Err(ApiError {
                status: StatusCode::UNAUTHORIZED,
                message: "This listener stopped.".into(),
            });
        }
        let batch = state.agents.attention.since(query.cursor.as_deref());
        if query.cursor.as_deref() != Some(&batch.cursor) || tokio::time::Instant::now() >= until {
            return Ok(Json(batch));
        }
        tokio::select! {
            _ = wait_revoked(&mut revoked) => {},
            _ = changes.recv() => {},
            _ = tokio::time::sleep_until(until) => {},
        }
    }
}

pub async fn agent_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: String,
) -> Result<Response, ApiError> {
    let session = find(&state, &id)?;
    let reply = crate::routes::blocking(move || Ok(handle(&state, &session, &body)))
        .await?
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(match reply {
        Some(reply) => Json(reply).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    })
}

/// A subagent's own conversation as chat items (`{start, items}`), or null
/// until the CLI has written its transcript. Read-only.
pub async fn agent_task_transcript(
    State(state): State<AppState>,
    Path((id, task_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let session = find(&state, &id)?;
    crate::routes::blocking(move || {
        Ok(match session.task_transcript(&task_id) {
            Some((start, items)) => json!({"start": start, "items": items}),
            None => Value::Null,
        })
    })
    .await
    .map(Json)
}

fn find(state: &AppState, id: &str) -> Result<Arc<AgentSession>, ApiError> {
    state.agents.get(id).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("no chat session {id}"),
    })
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
    let live = state.agents.has_linked_claude(&q.claude_account_id);
    Ok(Json(
        state.usage.get(q.claude_account_id, q.fresh, live).await?,
    ))
}

#[derive(Debug, Deserialize)]
pub struct AttachQuery {
    /// `changed`: `update` frames carry `meta` only when it changed. Clients
    /// that don't say so get it on every one, as they always have.
    meta: Option<String>,
}

pub async fn agent_attach(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(auth): Query<WsAuthQuery>,
    Query(opts): Query<AttachQuery>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    crate::auth::authorize_ws(&headers, auth.token.as_deref(), &state)?;
    // A browser socket can't read an HTTP status: say "not running" in the close.
    let Ok(session) = find(&state, &id) else {
        return Ok(ws.on_upgrade(|socket| {
            ws_close(
                socket,
                Some(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: SESSION_GONE,
                    reason: "not running".into(),
                }))),
            )
        }));
    };
    let revoked = state.revoked.clone();
    let every_meta = opts.meta.as_deref() != Some("changed");
    // A prompt can carry 10 images of up to ~6.7 MB base64 each and 5 PDFs of
    // ~13.4 MB; the default 16 MiB frame limit would drop the socket instead
    // of the prompt.
    Ok(ws
        .max_frame_size(MAX_PROMPT_BYTES)
        .max_message_size(MAX_PROMPT_BYTES)
        .on_upgrade(move |socket| stream(socket, state, session, revoked, every_meta)))
}

const MAX_PROMPT_BYTES: usize = 160 * 1024 * 1024;
/// Client messages waiting behind a slow one; a client sending more is dropped.
const MAX_QUEUED_MESSAGES: usize = 32;
/// The close code of a socket opened on a session that isn't running.
pub const SESSION_GONE: u16 = 4404;

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
    /// Carry a Claude chat on under another Claude account (`None`: the default).
    #[serde(rename_all = "camelCase")]
    Account {
        #[serde(default)]
        account_id: Option<String>,
    },
    Model {
        model: String,
    },
    Effort {
        effort: String,
    },
    /// Refresh the prompt cache now (a hidden keep-alive turn).
    CachePing,
    /// Restart a Claude terminal session's `claude`, a stuck turn included.
    Restart,
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
    state: &AppState,
    session: &Arc<AgentSession>,
    text: &str,
) -> anyhow::Result<Option<Value>> {
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
                        MAX_PROMPT_BYTES
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
        ClientMsg::Rewind {
            message_id,
            code,
            conversation,
            dry_run,
        } => {
            let result = rewind(state, session, &message_id, code, conversation, dry_run);
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
        // A Claude terminal session restarts in the mode; Codex switches in
        // place. A desktop native terminal's `claude` can't be restarted from here.
        ClientMsg::Mode { mode } if session.kind == AgentKind::Claude => {
            if state.agents.own_terminal(session).is_some() {
                state.agents.mode_terminal(&state.terminals, session, &mode)
            } else {
                anyhow::bail!("Switch it in the terminal with Shift+Tab.")
            }
        }
        ClientMsg::Mode { mode } => session.set_mode(&mode),
        ClientMsg::Account { account_id } if session.kind == AgentKind::Claude => {
            if state.agents.own_terminal(session).is_some() {
                state
                    .agents
                    .account_terminal(&state.terminals, session, account_id)
            } else {
                anyhow::bail!("Switch accounts by resuming this session in a new Claude pane.")
            }
        }
        ClientMsg::Account { .. } => anyhow::bail!("only Claude chats switch accounts"),
        ClientMsg::Model { model } => {
            // The plugin's stand-in list has no ids: ask the CLI for its own once.
            if !session.resolves_model(&model) {
                let (account, cwd) = (session.claude_account_id(), session.cwd());
                let fresh = tokio::runtime::Handle::current()
                    .block_on(state.models.refresh(account.clone(), cwd.clone()));
                if let Ok(models) = fresh {
                    state.agents.pin_models_for(&account, &cwd, &models);
                }
            }
            state.agents.set_model(session, &model)
        }
        ClientMsg::Effort { effort } => state.agents.set_effort(session, &effort),
        ClientMsg::CachePing => session.keep_cache_warm(),
        ClientMsg::Restart => state.agents.restart_session(&state.terminals, session),
        ClientMsg::CachePolicy { policy } => state.agents.set_cache_policy(session, policy),
    };
    reply.map(|()| None)
}

/// A plugin has no way to restore Claude's file checkpoints: a code rewind
/// answers `canRewind: false`, which the rewind panel shows as its own note,
/// and a real one is refused before the conversation is touched.
const NO_FILE_REWIND: &str =
    "files can't be restored in a terminal chat yet. Rewind the conversation only, or undo the changes with git.";

fn rewind(
    state: &AppState,
    session: &Arc<AgentSession>,
    message_id: &str,
    code: bool,
    conversation: bool,
    dry_run: bool,
) -> anyhow::Result<Option<Value>> {
    if session.kind != AgentKind::Claude {
        anyhow::bail!("Codex chats can't rewind");
    }
    if !workbench_core::claude_transcript::is_uuid(message_id) {
        anyhow::bail!("message id must be a UUID");
    }
    if code && !dry_run {
        anyhow::bail!("{NO_FILE_REWIND}");
    }
    let files = code.then(|| json!({"canRewind": false, "error": NO_FILE_REWIND}));
    if dry_run {
        return Ok(files);
    }
    if conversation {
        state
            .agents
            .rewind_terminal(&state.terminals, session, message_id)?;
    }
    Ok(files)
}

async fn stream(
    mut socket: WebSocket,
    state: AppState,
    session: Arc<AgentSession>,
    mut revoked: watch::Receiver<bool>,
    every_meta: bool,
) {
    let Some((snapshot, mut rx)) = subscribe(&session).await else {
        return;
    };
    let already_exited = session.has_exited();
    if !ws_send(&mut socket, Message::Text(snapshot)).await {
        return;
    }
    if already_exited {
        // A finished session has nothing more to say; holding the socket open
        // would pin it (and its transcript) in memory.
        ws_close(socket, None).await;
        return;
    }
    let mut heartbeat = crate::state::Heartbeat::new();
    // Client messages run one at a time, in order, on the blocking pool; the
    // loop keeps serving pings, pongs and a revoke while one does (a restart
    // waits up to 30s for its new `claude`). Session frames wait until its reply
    // is sent: a rewind's reply must come before the `replaced` that closes
    // the socket.
    let mut queued = std::collections::VecDeque::<String>::new();
    let mut running = None;
    loop {
        if running.is_none() {
            if let Some(text) = queued.pop_front() {
                // Restarts (a rewind, a mode switch), saving attachments and the
                // task-output directory walk block, so keep them off the async workers.
                let (worker, state) = (session.clone(), state.clone());
                running = Some(tokio::task::spawn_blocking(move || {
                    handle(&state, &worker, &text)
                }));
            }
        }
        tokio::select! {
            result = async { running.as_mut().expect("guarded").await }, if running.is_some() => {
                running = None;
                let frame = match result {
                    Ok(Ok(reply)) => reply,
                    Ok(Err(e)) => Some(json!({"t": "error", "message": e.to_string()})),
                    Err(e) => Some(json!({"t": "error", "message": e.to_string()})),
                };
                if let Some(frame) = frame {
                    if !ws_send(&mut socket, Message::Text(frame.to_string())).await {
                        return;
                    }
                }
            }
            alive = heartbeat.due(true) => {
                if !alive || !ws_send(&mut socket, Message::Ping(Vec::new())).await {
                    return;
                }
            }
            frame = rx.recv(), if running.is_none() => {
                let (frame, ended) = match frame {
                    Ok(frame) => (frame.render(every_meta).into_owned(), frame.ends()),
                    // Fell behind: start over from a fresh snapshot.
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let Some((snapshot, fresh)) = subscribe(&session).await else {
                            return;
                        };
                        rx = fresh;
                        (snapshot, false)
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                if ended {
                    ws_close(socket, Some(Message::Text(frame))).await;
                    return;
                }
                if !ws_send(&mut socket, Message::Text(frame)).await {
                    return;
                }
            }
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(text))) => {
                    heartbeat.heard();
                    if queued.len() >= MAX_QUEUED_MESSAGES {
                        ws_close(socket, None).await;
                        return;
                    }
                    queued.push_back(text);
                }
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                Some(Ok(_)) => heartbeat.heard(),
            },
            _ = wait_revoked(&mut revoked) => {
                ws_close(socket, Some(Message::Text(r#"{"t":"revoked"}"#.to_string()))).await;
                return;
            }
        }
    }
}

/// The snapshot is built under the session's driver lock: off the async workers.
async fn subscribe(
    session: &Arc<AgentSession>,
) -> Option<(String, broadcast::Receiver<Arc<crate::agent::Frame>>)> {
    let session = session.clone();
    tokio::task::spawn_blocking(move || session.subscribe())
        .await
        .ok()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ModGrant;
    use std::time::Instant;

    /// A session's driver lock is held for as long as its holder needs (a
    /// snapshot of a long chat): the list must not wait for it on an async
    /// worker, which would stall every other request on that worker.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn the_session_list_doesnt_block_a_worker_on_a_busy_session() {
        let state = AppState::new(
            crate::Managers::default(),
            None,
            tokio::sync::watch::channel(false).1,
        );
        let token = state
            .agents
            .grant_mod(ModGrant {
                pane_id: None,
                project_path: "/tmp".into(),
                worktree_path: None,
                claude_account_id: None,
                cwd: "/tmp".into(),
                resume_at: None,
                permission_mode: None,
                terminal_id: None,
                session_ids: Vec::new(),
            })
            .unwrap();
        let session = state
            .agents
            .attach_mod(&token, "0d6f2b1e-3c4a-4b5d-8e9f-a0b1c2d3e4f5")
            .unwrap();
        let (held_tx, held_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _driver = session.hold_driver();
            held_tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(600));
        });
        held_rx.recv().unwrap();

        let list = tokio::spawn(agent_list(State(state.clone())));
        let started = Instant::now();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            started.elapsed() < Duration::from_millis(400),
            "the worker was blocked for {:?}",
            started.elapsed()
        );
        let Ok(Json(listed)) = list.await.unwrap() else {
            panic!("the list failed");
        };
        assert_eq!(listed.len(), 1, "answers once the lock is free");
        holder.join().unwrap();
    }
}
