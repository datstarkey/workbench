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
//! raw PTY bytes as binary frames. Server→client control frames (text JSON):
//! `{"t":"takeover"}` — another client has attached (epoch bumped, old socket
//! will be closed); `{"t":"exit","code":<n|null>}` — shell exited.
//!
//! Single-attacher lease: only ONE client may drive input at a time. When a new
//! client attaches the server:
//!   1. Bumps the attacher epoch (watch channel).
//!   2. Sends `{"t":"takeover"}` to the OLD socket and closes it.
//!   3. The new client's attach loop detects its epoch matches the current one
//!      and proceeds normally (input accepted, output streamed).
//!
//! Gated by the same bearer auth as every other route. NOTE: browser WebSocket
//! can't send an `Authorization` header, so query-param auth for the WS is
//! supported via `?token=`.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Backstop against runaway terminal creation (see RemoteControlManager).
/// Overridable via `WORKBENCH_MAX_TERMINALS` (defaults to 64).
fn max_terminals() -> usize {
    std::env::var("WORKBENCH_MAX_TERMINALS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(64)
}

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
    /// Flips to `true` when the shell exits (PTY EOF) or the session is killed.
    /// Attached sockets select on this so they close instead of freezing — the
    /// broadcast `tx` lives inside this same Arc, so `rx.recv()` can never observe
    /// `Closed` from within the attach loop.
    done_tx: watch::Sender<bool>,
    /// Single-attacher lease: monotonically increasing epoch counter (atomic, so it
    /// can be read without holding a lock). Each `attach()` call fetch-adds 1. Input
    /// is only accepted when the attacher's ticket matches the current epoch (no newer
    /// client has attached).
    attacher_epoch: AtomicU64,
    /// Notification channel: carries the epoch value so an old attacher's
    /// `epoch_rx.changed()` fires when a new client takes over. A background receiver
    /// (`_epoch_rx_keeper`) keeps the Sender live (watch::Sender::send() is a no-op
    /// when there are zero receivers, which would break the epoch counter).
    attacher_kick_tx: watch::Sender<u64>,
    /// Kept alive purely so `attacher_kick_tx.send()` never sees zero receivers.
    _epoch_rx_keeper: watch::Receiver<u64>,
    /// Real shell exit code, captured by the reader thread when it reaps the child
    /// on PTY EOF (try_wait at exit-frame time often races ahead of the reap and
    /// returns None). Read by `exit_frame`.
    exit_code: Mutex<Option<i64>>,
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

    #[allow(clippy::too_many_arguments)]
    pub fn create(
        &self,
        cwd: String,
        name: Option<String>,
        command: Option<String>,
        cols: u16,
        rows: u16,
        pane_id: Option<String>,
        hook_socket: Option<String>,
        shell: Option<String>,
    ) -> anyhow::Result<TerminalMeta> {
        let max = max_terminals();
        if lock(&self.inner).len() >= max {
            anyhow::bail!("terminal session limit reached ({max})");
        }

        let pty = native_pty_system();
        let pair = pty.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // Prefer the caller's shell (desktop forwards the project's configured
        // shell for parity with the local PtyManager path); fall back to $SHELL.
        let shell_path = match shell {
            Some(s) if !s.is_empty() => s,
            _ => std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
        };
        let mut cmd = CommandBuilder::new(&shell_path);
        cmd.arg("-l");
        cmd.cwd(&cwd);
        for key in ["PATH", "HOME", "USER", "LANG", "SHELL", "LOGNAME"] {
            if let Ok(val) = std::env::var(key) {
                cmd.env(key, val);
            }
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        if let Some(id) = &pane_id {
            cmd.env("WORKBENCH_PANE_ID", id);
        }
        if let Some(sock) = &hook_socket {
            cmd.env("WORKBENCH_HOOK_SOCKET", sock);
        }

        // Shell integration (OSC 133): when launching a bare zsh (no startup
        // command), point ZDOTDIR at our generated rc dir so prompt/command marks
        // are emitted. Mirrors the desktop PtyManager path; the dir resolver lives
        // in workbench-core so the server can call it directly (no frontend seam).
        if command.is_none() && shell_path.contains("zsh") {
            if let Ok(zsh_dir) = workbench_core::shell_integration::ensure_shell_integration_dir() {
                if let Ok(orig) = std::env::var("ZDOTDIR") {
                    cmd.env("WORKBENCH_ORIG_ZDOTDIR", orig);
                } else if let Ok(home) = std::env::var("HOME") {
                    cmd.env("WORKBENCH_ORIG_ZDOTDIR", home);
                }
                cmd.env("ZDOTDIR", zsh_dir.to_string_lossy().as_ref());
            }
        }

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
        let (attacher_kick_tx, _epoch_rx_keeper) = watch::channel::<u64>(0);

        let session = Arc::new(TerminalSession {
            meta: Mutex::new(meta.clone()),
            writer: Mutex::new(writer),
            master: Mutex::new(master),
            child: Mutex::new(child),
            buffer: Mutex::new(VecDeque::new()),
            tx,
            done_tx,
            attacher_epoch: AtomicU64::new(0),
            attacher_kick_tx,
            _epoch_rx_keeper,
            exit_code: Mutex::new(None),
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
                // Shell exited (PTY EOF). Sweep the process group (SIGTERM the children
                // BEFORE the leader is reaped, so killpg never targets a freed PID) and
                // capture the leader's real exit code in the same pass, publishing it
                // before waking sockets so the exit frame carries the code.
                terminate_process_group(&session);
                lock(&session.meta).alive = false;
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
        // Remove under the map lock, then DROP the guard before the kill syscall +
        // wake — the outer map lock must never be held during I/O (it would serialize
        // create/list/attach against every kill).
        let session = lock(&self.inner).remove(id);
        match session {
            Some(s) => {
                // Wake attached sockets immediately (the reader thread's EOF signal can
                // race or be missed if the child is killed before producing EOF).
                let _ = s.done_tx.send(true);
                // Tear down the whole process GROUP (shell + descendants) on a detached
                // thread so this async route returns at once — the SIGTERM→grace→SIGKILL
                // escalation must not block a tokio worker.
                std::thread::spawn(move || terminate_process_group(&s));
                true
            }
            None => false,
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
    /// Forwarded as `WORKBENCH_PANE_ID` env var into the shell so hook scripts
    /// can identify which terminal pane they belong to.
    pub pane_id: Option<String>,
    /// Forwarded as `WORKBENCH_HOOK_SOCKET` env var — path/address of the hook
    /// socket the desktop sets up for `claude --hook` callbacks.
    pub hook_socket: Option<String>,
    /// Shell to launch (desktop forwards the project's configured shell). Empty /
    /// absent falls back to `$SHELL`.
    pub shell: Option<String>,
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
        terminals.create(
            cwd,
            body.name,
            body.command,
            body.cols,
            body.rows,
            body.pane_id,
            body.hook_socket,
            body.shell,
        )
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

/// Build the `{"t":"exit","code":<n|null>}` control frame, reading the child's real
/// exit status if it has been reaped. Falls back to `null` when the status isn't yet
/// available (e.g. PTY EOF observed a moment before the child is fully reaped).
fn exit_frame(session: &TerminalSession) -> Message {
    // Prefer the code the reader thread captured when it reaped the child; fall back
    // to a best-effort try_wait (the child may have just become reapable).
    //
    // Copy the recorded code out and drop the exit_code guard BEFORE touching the
    // child lock: terminate_process_group acquires child→exit_code, so holding
    // exit_code across the try_wait here would be a lock-order inversion (deadlock).
    let recorded = *lock(&session.exit_code);
    let code = recorded.or_else(|| match lock(&session.child).try_wait() {
        Ok(Some(status)) => Some(status.exit_code() as i64),
        _ => None,
    });
    let json = match code {
        Some(c) => format!(r#"{{"t":"exit","code":{c}}}"#),
        None => r#"{"t":"exit","code":null}"#.to_string(),
    };
    Message::Text(json)
}

/// Terminate the shell AND its descendants, capturing the leader's exit code into
/// `session.exit_code` when it is reaped. The PTY slave calls `setsid()`, so the
/// child PID is its process-group id; signalling the GROUP reaps detached children
/// (a backgrounded `vite &`, language servers) that a single-PID kill would orphan.
///
/// Signal-BEFORE-reap is load-bearing: the group is signalled while the leader still
/// holds the pgid, so `killpg` never targets a freed (and possibly recycled) PID.
/// Mirrors the desktop PtyManager path. Best-effort; the SIGTERM→grace→SIGKILL
/// escalation blocks, so callers run it on a dedicated thread or the reader thread.
#[cfg(unix)]
fn terminate_process_group(session: &TerminalSession) {
    use std::time::{Duration, Instant};
    let pgid = match lock(&session.child).process_id() {
        Some(pid) => pid as libc::pid_t,
        None => return,
    };
    // SIGHUP + SIGTERM the whole group FIRST — before any try_wait reaps the leader
    // and frees its PID. On a natural exit the leader is already a zombie (still
    // occupying the pgid), so this reaches surviving children without racing a reap.
    unsafe {
        libc::killpg(pgid, libc::SIGHUP);
        libc::killpg(pgid, libc::SIGTERM);
    }
    // Wait briefly for graceful exit (reaping the leader for its real code), then
    // SIGKILL anything still alive.
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        // Bind the try_wait result so the child guard (a scrutinee temporary that
        // would otherwise live for the whole match) is dropped before we sleep or
        // take the exit_code lock — exit_frame takes exit_code first, so holding
        // child across it would invert the lock order.
        let reaped = lock(&session.child).try_wait();
        match reaped {
            Ok(Some(status)) => {
                let mut code = lock(&session.exit_code);
                if code.is_none() {
                    *code = Some(status.exit_code() as i64);
                }
                return;
            }
            Ok(None) if Instant::now() >= deadline => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => break,
        }
    }
    unsafe {
        libc::killpg(pgid, libc::SIGKILL);
    }
}

#[cfg(windows)]
fn terminate_process_group(session: &TerminalSession) {
    // taskkill /T terminates the PID and its whole descendant tree.
    let pid = lock(&session.child).process_id();
    match pid {
        Some(pid) => {
            let _ = std::process::Command::new("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .output();
        }
        None => return,
    }
    let reaped = lock(&session.child).try_wait();
    if let Ok(Some(status)) = reaped {
        let mut code = lock(&session.exit_code);
        if code.is_none() {
            *code = Some(status.exit_code() as i64);
        }
    }
}

async fn attach(mut socket: WebSocket, session: Arc<TerminalSession>) {
    // --- Single-attacher lease -------------------------------------------------
    // Subscribe to the kick channel BEFORE bumping the epoch. tokio's `watch` marks
    // the value present at subscribe time as "seen", so a receiver created AFTER our
    // own send could miss a concurrent later attacher's send and then block on
    // `changed()` forever — leaving the old socket attached and violating the
    // single-attacher invariant. Subscribing first guarantees we observe every send
    // that follows, including our own (filtered out in the `changed()` arm below).
    let mut epoch_rx = session.attacher_kick_tx.subscribe();
    // Fetch-add the epoch atomically. The NEW value is our ticket; the OLD value was
    // held by any previously attached client.
    let my_epoch = session.attacher_epoch.fetch_add(1, Ordering::SeqCst) + 1;
    // Notify any previously attached client so its `epoch_rx.changed()` fires and it
    // closes. The _epoch_rx_keeper receiver keeps the channel alive, so send() never
    // returns Err (it is a no-op only when there are zero receivers).
    let _ = session.attacher_kick_tx.send(my_epoch);

    // --- Scrollback replay -----------------------------------------------------
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
        // Already dead: history is replayed; send exit frame then close.
        let _ = socket.send(exit_frame(&session)).await;
        let _ = socket.close().await;
        return;
    }

    loop {
        tokio::select! {
            // Epoch changed → our own send fires this once; a LATER attacher's send
            // means we've been displaced. The atomic is the source of truth.
            _ = epoch_rx.changed() => {
                if session.attacher_epoch.load(Ordering::SeqCst) == my_epoch {
                    // Our own epoch notification — we are still the current attacher.
                    continue;
                }
                // A newer client has attached; we are the old one. Send the takeover
                // control frame so the client knows it was displaced, then close the
                // socket. Do NOT kill the PTY — it keeps running for the new attacher.
                let _ = socket
                    .send(Message::Text(r#"{"t":"takeover"}"#.to_string()))
                    .await;
                // Send a WS Close frame so the client can distinguish a clean kick from
                // a dropped connection.
                let _ = socket.close().await;
                return;
            }
            _ = done_rx.changed() => {
                // Shell exited or the session was killed → send exit frame (with the
                // child's real exit code when available) then close so the client
                // surfaces the end of the session instead of freezing.
                let _ = socket.send(exit_frame(&session)).await;
                // Explicit close so the client sees a proper WS close frame.
                let _ = socket.close().await;
                return;
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
                        // Only accept input from the current attacher (epoch guard).
                        if session.attacher_epoch.load(Ordering::SeqCst) != my_epoch {
                            // We've been superseded — our epoch_rx.changed() arm will
                            // fire shortly and clean up; ignore this input.
                        } else if let Ok(msg) = serde_json::from_str::<ClientMsg>(&t) {
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
                        // Only accept raw binary input from the current attacher.
                        if session.attacher_epoch.load(Ordering::SeqCst) == my_epoch {
                            let mut w = lock(&session.writer);
                            let _ = w.write_all(&b);
                            let _ = w.flush();
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
    // Detach only — the shell keeps running so the session can be resumed.
}
