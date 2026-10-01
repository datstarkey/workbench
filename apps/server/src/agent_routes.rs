//! HTTP + WebSocket surface for chat sessions:
//! - `POST /agent/claude` starts (or returns) the session for a Claude session id.
//! - `DELETE /agent/claude/:id` stops it; `DELETE /agent/claude?paneId=` stops
//!   whatever a closed pane owned.
//! - `WS /agent/claude/:id/ws` streams `snapshot` then `update`/`exit` frames and
//!   takes `prompt` / `approve` / `interrupt` / `mode` messages. Any number of
//!   clients may attach; the first answer to an approval wins.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{broadcast, watch};
use workbench_core::claude_transcript::ApprovalDecision;

use crate::agent::{AgentSession, PromptImage, StartAgent, MAX_IMAGES};
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
}

pub async fn agent_start(
    State(state): State<AppState>,
    Json(body): Json<StartBody>,
) -> ApiResult<Json<Value>> {
    let agents = state.agents.clone();
    crate::routes::blocking(move || {
        // Sandboxed Claude runs through srt's launcher; chat mode spawns the CLI
        // directly, so refuse rather than silently run it unsandboxed.
        if workbench_core::config::load_workbench_settings()?.sandbox_runtime_enabled {
            anyhow::bail!(
                "Chat mode doesn't run inside the sandbox runtime yet. Use the terminal, or turn the sandbox off in Settings."
            );
        }
        let registered: Vec<String> = workbench_core::config::load_projects()?
            .into_iter()
            .map(|p| p.path)
            .collect();
        let cwd = RemoteControlManager::resolve_cwd(
            &body.project_path,
            body.worktree_path.as_deref(),
            &registered,
        )?;
        let config_dir =
            workbench_core::claude_accounts::resolve_saved(body.claude_account_id.as_deref())?;
        let session = agents.start(StartAgent {
            cwd,
            session_id: body.session_id,
            permission_mode: body.permission_mode,
            pane_id: body.pane_id,
            hook_socket: body.hook_socket,
            config_dir,
        })?;
        Ok(json!({"sessionId": session.id()}))
    })
    .await
    .map(Json)
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

pub async fn agent_attach(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(auth): Query<WsAuthQuery>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    crate::auth::authorize_ws(&headers, auth.token.as_deref(), &state)?;
    let session = state.agents.get(&id).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("no chat session {id}"),
    })?;
    let revoked = state.revoked.clone();
    Ok(ws.on_upgrade(move |socket| stream(socket, session, revoked)))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
enum ClientMsg {
    Prompt {
        #[serde(default)]
        text: String,
        #[serde(default)]
        images: Vec<PromptImage>,
    },
    #[serde(rename_all = "camelCase")]
    Approve {
        request_id: String,
        decision: ApprovalDecision,
        /// `AskUserQuestion` answers: question text → chosen label or own words.
        #[serde(default)]
        answers: Option<serde_json::Map<String, Value>>,
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
        ClientMsg::Prompt { text, images } if text.trim().is_empty() && images.is_empty() => Ok(()),
        ClientMsg::Prompt { text, images } => {
            if images.len() > MAX_IMAGES {
                anyhow::bail!("attach at most {MAX_IMAGES} images per message");
            }
            images.iter().try_for_each(PromptImage::validate)?;
            session.prompt(&text, &images)
        }
        ClientMsg::Approve {
            request_id,
            decision,
            answers,
        } => session.approve(&request_id, decision, answers.as_ref()),
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
    if socket.send(Message::Text(snapshot)).await.is_err() {
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
                if socket.send(Message::Text(frame)).await.is_err() {
                    return;
                }
            }
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(text))) => {
                    let frame = match handle(&session, &text) {
                        Ok(reply) => reply,
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
