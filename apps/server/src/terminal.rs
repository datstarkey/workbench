//! Persistent PTY terminals, streamed over WebSocket.
//!
//! A terminal is a login shell running in a project / worktree directory inside
//! a PTY on the server. Unlike a raw proxy, sessions **persist across WebSocket
//! disconnects**: closing the mobile terminal view detaches but leaves the shell
//! running, so it can be resumed (with scrollback replayed from a ring buffer).
//!
//! REST + WS:
//!   - `GET  /remote/terminals`        → list sessions
//!   - `POST /remote/terminals`        → create a session, returns its metadata
//!   - `GET  /remote/terminals/:id/ws` → attach (replays buffer, then streams)
//!   - `DELETE /remote/terminals/:id`  → kill a session
//!
//! WS wire protocol (same as before): client→server JSON text
//! (`{"t":"i","d":..}` input, `{"t":"r","c":..,"r":..}` resize); server→client
//! raw PTY bytes as binary frames.
//!
//! Gated by the same bearer auth as every other route. NOTE: browser WebSocket
//! can't send an `Authorization` header, so query-param auth for the WS is a
//! follow-up; the default deployment is network-secured (e.g. Tailscale).

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    response::Response,
    Json,
};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};

use crate::error::{ApiError, ApiResult};
use crate::spawn::RemoteControlManager;
use crate::state::AppState;

/// Scrollback kept per session for replay on reattach.
const BUFFER_CAP: usize = 256 * 1024;

/// Backstop against runaway terminal creation (see MAX_REMOTE_SESSIONS).
const MAX_TERMINALS: usize = 64;

fn default_cols() -> u16 {
    80
}
fn default_rows() -> u16 {
    24
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalMeta {
    pub id: String,
    pub name: Option<String>,
    pub cwd: String,
    /// Unix epoch milliseconds.
    pub created_at: u64,
    pub alive: bool,
}

struct TerminalSession {
    meta: Mutex<TerminalMeta>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    /// Ring buffer of recent output, replayed when a client (re)attaches.
    buffer: Mutex<VecDeque<u8>>,
    /// Live output fan-out to all attached clients.
    tx: broadcast::Sender<Vec<u8>>,
    /// Flips to `false` when the shell exits (PTY EOF) or the session is killed.
    /// Attached sockets select on this so they close instead of freezing — the
    /// broadcast `tx` lives inside this same Arc, so `rx.recv()` can never observe
    /// `Closed` from within the attach loop.
    done_tx: watch::Sender<bool>,
}

#[derive(Clone, Default)]
pub struct TerminalManager {
    inner: Arc<Mutex<HashMap<String, Arc<TerminalSession>>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl TerminalManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(
        &self,
        cwd: String,
        name: Option<String>,
        command: Option<String>,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<TerminalMeta> {
        if lock(&self.inner).len() >= MAX_TERMINALS {
            anyhow::bail!("terminal session limit reached ({MAX_TERMINALS})");
        }

        let pty = native_pty_system();
        let pair = pty.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.arg("-l");
        cmd.cwd(&cwd);
        for key in ["PATH", "HOME", "USER", "LANG", "SHELL", "LOGNAME"] {
            if let Ok(val) = std::env::var(key) {
                cmd.env(key, val);
            }
        }
        cmd.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let master = pair.master;
        let reader = master.try_clone_reader()?;
        let writer = master.take_writer()?;

        let id = uuid::Uuid::new_v4().to_string();
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let meta = TerminalMeta {
            id: id.clone(),
            name,
            cwd,
            created_at,
            alive: true,
        };
        let (tx, _rx) = broadcast::channel::<Vec<u8>>(1024);
        let (done_tx, _done_rx) = watch::channel(false);

        let session = Arc::new(TerminalSession {
            meta: Mutex::new(meta.clone()),
            writer: Mutex::new(writer),
            master: Mutex::new(master),
            child: Mutex::new(child),
            buffer: Mutex::new(VecDeque::new()),
            tx,
            done_tx,
        });

        // Drain the PTY on a blocking thread: append to the replay buffer and
        // fan out to attached clients. On EOF mark the session not-alive.
        {
            let session = session.clone();
            let mut reader = reader;
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let chunk = buf[..n].to_vec();
                            // Append to the replay buffer and fan out to attached
                            // clients while holding the buffer lock, so a client that
                            // attaches (and subscribes under the same lock) sees each
                            // chunk in exactly one of {replay, live stream}.
                            let mut b = lock(&session.buffer);
                            b.extend(chunk.iter().copied());
                            while b.len() > BUFFER_CAP {
                                b.pop_front();
                            }
                            let _ = session.tx.send(chunk);
                        }
                    }
                }
                lock(&session.meta).alive = false;
                // Wake attached sockets so they close instead of hanging on a dead shell.
                let _ = session.done_tx.send(true);
            });
        }

        // Optionally run an initial command (e.g. `claude`). PTY input is
        // buffered, so writing before the shell is ready is fine.
        if let Some(cmd) = &command {
            let mut w = lock(&session.writer);
            let _ = w.write_all(cmd.as_bytes());
            let _ = w.write_all(b"\n");
            let _ = w.flush();
        }

        lock(&self.inner).insert(id, session);
        Ok(meta)
    }

    pub fn list(&self) -> Vec<TerminalMeta> {
        let map = lock(&self.inner);
        // Reap exited shells so they stop showing as alive (and don't linger as
        // zombies). Sessions are kept in the map so they can still be inspected
        // / resumed until the user dismisses them.
        for s in map.values() {
            if matches!(lock(&s.child).try_wait(), Ok(Some(_))) {
                lock(&s.meta).alive = false;
            }
        }
        let mut out: Vec<TerminalMeta> = map.values().map(|s| lock(&s.meta).clone()).collect();
        out.sort_by_key(|m| m.created_at);
        out
    }

    fn get(&self, id: &str) -> Option<Arc<TerminalSession>> {
        lock(&self.inner).get(id).cloned()
    }

    pub fn kill(&self, id: &str) -> bool {
        if let Some(s) = lock(&self.inner).remove(id) {
            let _ = lock(&s.child).kill();
            // Wake attached sockets so they close (the reader thread's EOF signal can
            // race or be missed if the child was killed before producing EOF).
            let _ = s.done_tx.send(true);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTerminalBody {
    pub project_path: String,
    pub worktree_path: Option<String>,
    pub name: Option<String>,
    /// Optional command to run once the shell starts (e.g. `claude`).
    pub command: Option<String>,
    #[serde(default = "default_cols")]
    pub cols: u16,
    #[serde(default = "default_rows")]
    pub rows: u16,
}

pub async fn terminal_list(State(state): State<AppState>) -> ApiResult<Json<Vec<TerminalMeta>>> {
    Ok(Json(state.terminals.list()))
}

pub async fn terminal_create(
    State(state): State<AppState>,
    Json(body): Json<CreateTerminalBody>,
) -> ApiResult<Json<TerminalMeta>> {
    let terminals = state.terminals.clone();
    // openpty + fork/exec and the project-allowlist load are blocking — run them off
    // the async executor so a slow spawn doesn't stall a tokio worker thread.
    crate::routes::blocking(move || {
        let registered: Vec<String> = workbench_core::config::load_projects()?
            .into_iter()
            .map(|p| p.path)
            .collect();
        let cwd = RemoteControlManager::resolve_cwd(
            &body.project_path,
            body.worktree_path.as_deref(),
            &registered,
        )?;
        terminals.create(cwd, body.name, body.command, body.cols, body.rows)
    })
    .await
    .map(Json)
}

pub async fn terminal_kill(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    state.terminals.kill(&id);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "t")]
enum ClientMsg {
    #[serde(rename = "i")]
    Input { d: String },
    #[serde(rename = "r")]
    Resize { c: u16, r: u16 },
}

#[derive(Debug, Deserialize)]
pub struct WsAuthQuery {
    /// Browser WebSocket can't send an Authorization header, so the bearer token
    /// (when the server is started with one) is carried here instead.
    token: Option<String>,
}

pub async fn terminal_attach(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(auth): Query<WsAuthQuery>,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    // This route is exempt from the global bearer middleware (a browser WebSocket
    // can't set an Authorization header), so authenticate the ?token= query param
    // here — same token, same constant-time compare as every other route.
    if let Some(expected) = state.token.as_deref() {
        let ok = auth
            .token
            .as_deref()
            .is_some_and(|t| crate::auth::constant_time_eq(t.as_bytes(), expected.as_bytes()));
        if !ok {
            return Err(ApiError {
                status: StatusCode::UNAUTHORIZED,
                message: "unauthorized".to_string(),
            });
        }
    }

    let session = state
        .terminals
        .get(&id)
        .ok_or_else(|| anyhow::anyhow!("no terminal with id {id}"))?;
    Ok(ws.on_upgrade(move |socket| attach(socket, session)))
}

async fn attach(mut socket: WebSocket, session: Arc<TerminalSession>) {
    // Subscribe and snapshot the scrollback under the same buffer lock the reader
    // holds when it appends+broadcasts. This makes the handoff atomic: output
    // produced during attach can't slip between the snapshot and the subscription
    // (which would drop it) nor land in both (which would duplicate it).
    let (replay, mut rx) = {
        let buffer = lock(&session.buffer);
        let rx = session.tx.subscribe();
        let replay: Vec<u8> = buffer.iter().copied().collect();
        (replay, rx)
    };

    // Replay scrollback so a resumed terminal shows its history.
    if !replay.is_empty() && socket.send(Message::Binary(replay)).await.is_err() {
        return;
    }

    // Watch for shell-exit / kill so we close the socket instead of hanging: the
    // broadcast `tx` lives inside `session`, so `rx.recv()` can't see `Closed` here.
    let mut done_rx = session.done_tx.subscribe();
    if *done_rx.borrow_and_update() {
        // Already dead: history is replayed; close so the client sees it ended.
        return;
    }

    loop {
        tokio::select! {
            _ = done_rx.changed() => {
                // Shell exited or the session was killed → close the socket so the
                // client surfaces the end of the session instead of freezing.
                break;
            }
            out = rx.recv() => {
                match out {
                    Ok(bytes) => {
                        if socket.send(Message::Binary(bytes)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Client fell behind and missed output; resync from the
                        // scrollback buffer (clear screen + replay) rather than
                        // leaving the terminal corrupted.
                        let snap: Vec<u8> = lock(&session.buffer).iter().copied().collect();
                        let mut resync = Vec::with_capacity(snap.len() + 7);
                        resync.extend_from_slice(b"\x1b[2J\x1b[H");
                        resync.extend_from_slice(&snap);
                        if socket.send(Message::Binary(resync)).await.is_err() {
                            break;
                        }
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(msg) = serde_json::from_str::<ClientMsg>(&t) {
                            match msg {
                                ClientMsg::Input { d } => {
                                    let mut w = lock(&session.writer);
                                    let _ = w.write_all(d.as_bytes());
                                    let _ = w.flush();
                                }
                                ClientMsg::Resize { c, r } => {
                                    let _ = lock(&session.master).resize(PtySize {
                                        rows: r,
                                        cols: c,
                                        pixel_width: 0,
                                        pixel_height: 0,
                                    });
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Binary(b))) => {
                        let mut w = lock(&session.writer);
                        let _ = w.write_all(&b);
                        let _ = w.flush();
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
    // Detach only — the shell keeps running so the session can be resumed.
}
