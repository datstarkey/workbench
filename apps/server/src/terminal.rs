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
//!   - `DELETE /remote/terminals/:id`  → kill a session (`?wait=true`: respond
//!     only once its processes are gone)
//!
//! WS wire protocol (same as before): client→server JSON text
//! (`{"t":"i","d":..}` input, `{"t":"r","c":..,"r":..}` resize); server→client
//! raw PTY bytes as binary frames. Server→client control frames (text JSON):
//! `{"t":"takeover"}` — another client has attached (epoch bumped, old socket
//! will be closed); `{"t":"exit","code":<n|null>}` — shell exited;
//! `{"t":"revoked"}` — the listener this socket came through stopped (server
//! mode off / token rotated), so the socket is closed.
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
//! supported via `?token=`. The upgrade also checks `Origin` (see
//! `auth::ws_origin_allowed`).

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{HeaderMap, StatusCode},
    response::Response,
    Json,
};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Scrollback kept per session for replay on reattach.
const BUFFER_CAP: usize = 256 * 1024;

/// Shell-readiness window for the startup command (see `wait_for_shell_prompt`).
const STARTUP_FLOOR: Duration = Duration::from_millis(300);
const STARTUP_QUIET: Duration = Duration::from_millis(150);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_POLL: Duration = Duration::from_millis(20);

/// Backstop against runaway terminal creation.
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
    /// The Claude session this terminal's `claude` runs: a chat's terminal,
    /// listed before its plugin attaches, so clients adopt it as the chat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,
    /// On a create only: something the person should know about how it started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

struct TerminalSession {
    pane_id: Option<String>,
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
    /// Bumped when the list changes: a terminal opens, exits or is removed.
    changes: crate::changes::Changes,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl TerminalManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changes.subscribe()
    }

    pub fn terminal_for_pane(&self, pane_id: &str) -> Option<TerminalMeta> {
        lock(&self.inner)
            .values()
            .find(|s| s.pane_id.as_deref() == Some(pane_id))
            .map(|s| lock(&s.meta).clone())
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
        claude_config_dir: Option<&std::path::Path>,
        claude_session_id: Option<String>,
        extra_env: &[(&str, String)],
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
        // shell); fall back to the platform default.
        let shell_path = match shell {
            Some(s) if !s.is_empty() => s,
            _ => workbench_core::shell::default_shell(),
        };
        let mut cmd = CommandBuilder::new(&shell_path);
        for arg in workbench_core::shell::login_args() {
            cmd.arg(arg);
        }
        cmd.cwd(&cwd);
        for (key, val) in workbench_core::shell::inherited_env() {
            cmd.env(key, val);
        }
        if let Some(id) = &pane_id {
            cmd.env("WORKBENCH_PANE_ID", id);
        }
        if let Some(sock) = &hook_socket {
            cmd.env("WORKBENCH_HOOK_SOCKET", sock);
            if let Some(dirs) = workbench_core::claude_plugin::plugin_dirs_env() {
                cmd.env(workbench_core::claude_plugin::PLUGIN_DIRS_ENV, dirs);
            }
        }
        for (key, val) in extra_env {
            cmd.env(key, val);
        }
        // Set on the shell, so every `claude` run in this pane uses that login.
        if let Some(dir) = claude_config_dir {
            cmd.env(workbench_core::claude_accounts::CONFIG_DIR_ENV, dir);
        }
        // CommandBuilder inherits the whole server env; never hand the shell the
        // standalone server's bearer token.
        cmd.env_remove("WORKBENCH_TOKEN");

        // Shell integration (OSC 133): when launching a bare zsh (no startup
        // command), point ZDOTDIR at our generated rc dir so prompt/command marks
        // are emitted. The dir resolver lives in workbench-core so the server can
        // call it directly (no frontend seam).
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
            claude_session_id,
            notice: None,
        };
        let (tx, _rx) = broadcast::channel::<Vec<u8>>(1024);
        let (done_tx, _done_rx) = watch::channel(false);
        let (attacher_kick_tx, _epoch_rx_keeper) = watch::channel::<u64>(0);

        let session = Arc::new(TerminalSession {
            pane_id,
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
            let changes = self.changes.clone();
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
                changes.notify();
            });
        }

        // Optionally run an initial command (e.g. `claude`), once the shell is
        // actually reading input.
        if let Some(cmd) = command {
            let session = session.clone();
            std::thread::spawn(move || {
                wait_for_shell_prompt(&session);
                let mut w = lock(&session.writer);
                let _ = workbench_core::shell::submit_line(&mut **w, &cmd);
            });
        }

        lock(&self.inner).insert(id, session);
        self.changes.notify();
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

    /// The terminal's recent output (its replay buffer), lossily decoded.
    pub fn recent_output(&self, id: &str) -> Option<String> {
        let session = self.get(id)?;
        let buffer = lock(&session.buffer);
        let (head, tail) = buffer.as_slices();
        Some(String::from_utf8_lossy(&[head, tail].concat()).into_owned())
    }

    /// Type into the terminal as an attached client would.
    pub fn type_keys(&self, id: &str, keys: &[u8]) -> bool {
        let Some(session) = self.get(id) else {
            return false;
        };
        let mut w = lock(&session.writer);
        w.write_all(keys).and_then(|()| w.flush()).is_ok()
    }

    fn get(&self, id: &str) -> Option<Arc<TerminalSession>> {
        lock(&self.inner).get(id).cloned()
    }

    pub fn kill(&self, id: &str) -> bool {
        match self.remove(id) {
            Some(s) => {
                // Tear down the whole process GROUP (shell + descendants) on a detached
                // thread so this async route returns at once — the SIGTERM→grace→SIGKILL
                // escalation must not block a tokio worker.
                std::thread::spawn(move || terminate_process_group(&s));
                true
            }
            None => false,
        }
    }

    /// [`Self::kill`], returning only once the process group is gone — so a
    /// client can start another `claude` on the same session without two
    /// writers overlapping. Blocking.
    pub fn kill_and_wait(&self, id: &str) -> bool {
        let Some(s) = self.remove(id) else {
            return false;
        };
        #[cfg(unix)]
        let groups = {
            // Under job control the foreground command (`claude`) runs in its own
            // group, which the shell's group signal reaches only indirectly.
            let shell = lock(&s.child).process_id().map(|p| p as libc::pid_t);
            let foreground = lock(&s.master).process_group_leader();
            if let Some(fg) = foreground.filter(|&fg| Some(fg) != shell) {
                unsafe {
                    libc::killpg(fg, libc::SIGHUP);
                    libc::killpg(fg, libc::SIGTERM);
                }
            }
            [shell, foreground]
        };
        terminate_process_group(&s);
        // The shell can be reaped while the rest (Claude itself) is still exiting.
        #[cfg(unix)]
        wait_for_groups_exit(&groups.into_iter().flatten().collect::<Vec<_>>());
        true
    }

    /// Remove under the map lock, then DROP the guard before the wake — the outer
    /// map lock must never be held during I/O (it would serialize
    /// create/list/attach against every kill).
    fn remove(&self, id: &str) -> Option<Arc<TerminalSession>> {
        let session = lock(&self.inner).remove(id)?;
        // Wake attached sockets immediately (the reader thread's EOF signal can
        // race or be missed if the child is killed before producing EOF).
        let _ = session.done_tx.send(true);
        self.changes.notify();
        Some(session)
    }

    /// Kill every terminal and block until each process group is torn down. For
    /// app shutdown/relaunch: descendants that outlive the app keep its old macOS
    /// Dock tile alive and leave stray console windows on Windows.
    pub fn kill_all(&self) {
        let sessions: Vec<_> = lock(&self.inner).drain().map(|(_, s)| s).collect();
        self.changes.notify();
        let handles: Vec<_> = sessions
            .into_iter()
            .map(|s| {
                let _ = s.done_tx.send(true);
                std::thread::spawn(move || terminate_process_group(&s))
            })
            .collect();
        for handle in handles {
            let _ = handle.join();
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
    /// Run Claude on this session instead of `command`; the server builds the
    /// command so the sandbox wrapper and permission mode can't be skipped.
    pub claude_session: Option<ClaudeSessionLaunch>,
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
    /// absent falls back to the platform default (`workbench_core::shell`).
    pub shell: Option<String>,
    /// Saved Claude account whose config dir becomes the shell's
    /// `CLAUDE_CONFIG_DIR`. An id, never a path, so clients can't point it anywhere.
    pub claude_account_id: Option<String>,
}

pub use workbench_core::claude_launch::ClaudeSessionLaunch;

#[derive(Debug, Deserialize)]
pub struct KillQuery {
    #[serde(default)]
    wait: bool,
}

pub async fn terminal_list(State(state): State<AppState>) -> ApiResult<Json<Vec<TerminalMeta>>> {
    Ok(Json(state.terminals.list()))
}

pub async fn terminal_create(
    State(state): State<AppState>,
    Json(body): Json<CreateTerminalBody>,
) -> ApiResult<Json<TerminalMeta>> {
    if body.command.is_some() && body.claude_session.is_some() {
        return Err(ApiError::bad_request(
            "send either command or claudeSession, not both",
        ));
    }
    let terminals = state.terminals.clone();
    let agents = state.agents.clone();
    // openpty + fork/exec and the project-allowlist load are blocking — run them off
    // the async executor so a slow spawn doesn't stall a tokio worker thread.
    crate::routes::blocking(move || create_from_body(&terminals, &agents, body))
        .await
        .map(Json)
}

/// Create a terminal as `POST /remote/terminals` does (also how a Claude chat
/// starts: its terminal runs `claude` and the plugin makes it the chat).
/// Blocking.
pub fn create_from_body(
    terminals: &TerminalManager,
    agents: &crate::agent::AgentManager,
    mut body: CreateTerminalBody,
) -> anyhow::Result<TerminalMeta> {
    let cwd = crate::cwd::resolve_cwd(&body.project_path, body.worktree_path.as_deref())?;
    let claude_config_dir =
        workbench_core::claude_accounts::resolve_saved(body.claude_account_id.as_deref())?;
    // Resume whatever has a transcript, as a chat start does: a client can't
    // know whether its session ever got a message written.
    if let Some(session) = body.claude_session.as_mut() {
        session.resume =
            crate::agent::claude_history_exists(claude_config_dir.as_deref(), &session.id);
    }
    let notice = body
        .claude_session
        .as_ref()
        .and_then(workbench_core::claude_launch::prompt_notice);
    let command =
        workbench_core::claude_launch::startup_command(body.command, body.claude_session.as_ref())?;
    // The plugin reads no live mode until a hook reports one, so it's told the
    // one `claude` starts in rather than guessing the settings default. Read
    // before a token is granted: an error returns past the revoke below.
    let launch_mode = match body.claude_session.as_ref() {
        Some(session) => workbench_core::claude_launch::launch_mode(
            session.permission_mode.as_deref(),
            &workbench_core::config::load_workbench_settings()?,
        )
        .map(String::from),
        None => None,
    };
    // The Workbench plugin in this pane's `claude` runs the session as a
    // chat through `mod_routes`, with a token good for this terminal only.
    let (token, mod_env) = agents
        .mod_env(crate::agent::ModGrant {
            pane_id: body.pane_id.clone(),
            project_path: body.project_path.clone(),
            worktree_path: body.worktree_path.clone(),
            claude_account_id: body.claude_account_id.clone(),
            cwd: cwd.clone(),
            hook_socket: body.hook_socket.clone(),
            resume_at: body
                .claude_session
                .as_ref()
                .and_then(|s| s.resume_at.clone()),
            permission_mode: body
                .claude_session
                .as_ref()
                .and_then(|s| s.permission_mode.clone()),
            terminal_id: None,
        })?
        .unzip();
    let mut mod_env = mod_env.unwrap_or_default();
    if let (Some(mode), Some(_)) = (launch_mode, token.as_ref()) {
        mod_env.push(("WORKBENCH_PERMISSION_MODE", mode));
    }
    let created = terminals.create(
        cwd,
        body.name,
        command,
        body.cols,
        body.rows,
        body.pane_id,
        body.hook_socket,
        body.shell,
        claude_config_dir.as_deref(),
        // Only a terminal its plugin can attach becomes a chat.
        body.claude_session
            .as_ref()
            .filter(|_| token.is_some())
            .map(|s| s.id.clone()),
        &mod_env,
    );
    match (&created, &token) {
        (Ok(meta), Some(token)) => agents.set_grant_terminal(token, &meta.id),
        (Err(_), Some(token)) => agents.revoke_grant(token),
        _ => {}
    }
    created.map(|meta| TerminalMeta { notice, ..meta })
}

pub async fn terminal_kill(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<KillQuery>,
) -> ApiResult<StatusCode> {
    let (agents, terminals) = (state.agents.clone(), state.terminals.clone());
    crate::routes::blocking(move || {
        agents.end_terminal(&id);
        if q.wait {
            terminals.kill_and_wait(&id);
        } else {
            terminals.kill(&id);
        }
        Ok(())
    })
    .await?;
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
    pub(crate) token: Option<String>,
}

pub async fn terminal_attach(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(auth): Query<WsAuthQuery>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    crate::auth::authorize_ws(&headers, auth.token.as_deref(), &state)?;

    let session = state
        .terminals
        .get(&id)
        .ok_or_else(|| anyhow::anyhow!("no terminal with id {id}"))?;
    let revoked = state.revoked.clone();
    Ok(ws.on_upgrade(move |socket| attach(socket, session, revoked)))
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

fn revoked_frame() -> Message {
    Message::Text(r#"{"t":"revoked"}"#.to_string())
}

/// Block until a freshly spawned shell is ready for input: a settling delay,
/// then its startup output (banner, prompt) going quiet.
///
/// Windows is why this exists. cmd.exe and PSReadLine drain the console input
/// buffer while they initialise, so a startup command written into a shell that
/// has only just spawned is swallowed and the pane sits at an idle prompt — the
/// "startup command never ran" bug. Waiting for output to go quiet (rather than
/// sleeping a fixed interval) also covers a slow PowerShell profile; the floor
/// covers the ConPTY preamble, which arrives before the shell has printed
/// anything and would otherwise read as a prompt. Capped, so a shell that never
/// prints still gets its command.
fn wait_for_shell_prompt(session: &TerminalSession) {
    std::thread::sleep(STARTUP_FLOOR);

    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut last_len = 0;
    let mut quiet = Duration::ZERO;
    while Instant::now() < deadline {
        let len = lock(&session.buffer).len();
        if len != last_len {
            last_len = len;
            quiet = Duration::ZERO;
        } else if len > 0 && quiet >= STARTUP_QUIET {
            return;
        } else {
            quiet += STARTUP_POLL;
        }
        std::thread::sleep(STARTUP_POLL);
    }
}

/// Terminate the shell AND its descendants, capturing the leader's exit code into
/// `session.exit_code` when it is reaped. The PTY slave calls `setsid()`, so the
/// child PID is its process-group id; signalling the GROUP reaps detached children
/// (a backgrounded `vite &`, language servers) that a single-PID kill would orphan.
///
/// Signal-BEFORE-reap is load-bearing: the group is signalled while the leader still
/// holds the pgid, so `killpg` never targets a freed (and possibly recycled) PID.
/// Best-effort; the SIGTERM→grace→SIGKILL
/// escalation blocks, so callers run it on a dedicated thread or the reader thread.
#[cfg(unix)]
fn terminate_process_group(session: &TerminalSession) {
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

/// Poll until no process is left in `groups`, SIGKILLing stragglers after a
/// grace period. Safe after a leader is reaped: a pid is never reused while a
/// process group with that id still exists.
#[cfg(unix)]
fn wait_for_groups_exit(groups: &[libc::pid_t]) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let alive: Vec<_> = groups
            .iter()
            .copied()
            .filter(|&g| unsafe { libc::killpg(g, 0) } == 0)
            .collect();
        if alive.is_empty() {
            return;
        }
        if Instant::now() >= deadline {
            for g in alive {
                unsafe {
                    libc::killpg(g, libc::SIGKILL);
                }
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(windows)]
fn terminate_process_group(session: &TerminalSession) {
    // taskkill /T terminates the PID and its whole descendant tree.
    let pid = lock(&session.child).process_id();
    match pid {
        Some(pid) => {
            let _ = workbench_core::shell::command("taskkill")
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

async fn attach(
    mut socket: WebSocket,
    session: Arc<TerminalSession>,
    mut revoked: watch::Receiver<bool>,
) {
    // An upgrade that raced the listener stopping must not kick the live attacher.
    if *revoked.borrow_and_update() {
        let _ = socket.send(revoked_frame()).await;
        let _ = socket.close().await;
        return;
    }

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
            _ = crate::state::wait_revoked(&mut revoked) => {
                // The listener stopped (server mode off / token rotated): cut this
                // client off. The PTY keeps running for other listeners' clients.
                let _ = socket.send(revoked_frame()).await;
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
