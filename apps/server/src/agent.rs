//! Chat-mode Claude sessions: one `claude -p` process per session, driven over
//! stream-json on its stdin/stdout. Events fold into a
//! [`workbench_core::claude_transcript::Transcript`]; every change is broadcast
//! to attached clients (desktop chat pane, phone) as an `update` frame.
//!
//! A session id is the Claude session id, so the same conversation can move
//! between this and a terminal running `claude --resume <id>` — but only one
//! process may own it at a time; the client stops one before starting the other.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;
use workbench_core::claude_launch::PERMISSION_MODES;
use workbench_core::claude_transcript::{
    self, ApprovalDecision, RunningSummary, Transcript, TranscriptItem, WaitingSummary,
};

/// Effort levels `effortLevel` accepts.
const EFFORT_LEVELS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

const DEFAULT_MAX_AGENTS: usize = 16;

mod image;
pub use image::{PromptImage, MAX_IMAGES};
/// Items in an attach snapshot; older history stays on disk.
const SNAPSHOT_ITEMS: usize = 500;
const STOP_GRACE: Duration = Duration::from_secs(3);
/// How much of a background task's output the panel shows.
const TASK_OUTPUT_TAIL: u64 = 64 * 1024;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

pub struct StartAgent {
    pub cwd: String,
    /// As the client gave them, for listing; `cwd` is what they resolved to.
    pub project_path: String,
    pub worktree_path: Option<String>,
    pub session_id: String,
    pub permission_mode: Option<String>,
    /// Forwarded as `WORKBENCH_PANE_ID` / `WORKBENCH_HOOK_SOCKET` so hooks keep
    /// driving the desktop's activity tracking, as for terminal panes.
    pub pane_id: Option<String>,
    pub hook_socket: Option<String>,
    /// The Claude account's config dir (`CLAUDE_CONFIG_DIR`); `None` is the default login.
    pub config_dir: Option<PathBuf>,
    /// The id `config_dir` was resolved from, for listing.
    pub claude_account_id: Option<String>,
}

pub struct AgentSession {
    /// Changes when `/clear` continues the conversation under a new id.
    session_id: Mutex<String>,
    pub pane_id: Option<String>,
    project_path: String,
    worktree_path: Option<String>,
    claude_account_id: Option<String>,
    transcript: Mutex<Transcript>,
    /// Unix ms; both are written under the transcript lock.
    busy_since: Mutex<Option<u64>>,
    updated_at: AtomicU64,
    tx: broadcast::Sender<String>,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Child>,
    /// Kept apart from `child` so a stop never waits on the reaping lock.
    pid: u32,
    exited: AtomicBool,
    task_files: Mutex<HashMap<String, PathBuf>>,
}

/// One live session as the phone's home screen lists it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSummary {
    pub session_id: String,
    pub project_path: String,
    pub worktree_path: Option<String>,
    pub pane_id: Option<String>,
    pub claude_account_id: Option<String>,
    pub title: Option<String>,
    pub model: Option<String>,
    pub busy: bool,
    pub exited: bool,
    pub busy_since: Option<u64>,
    pub updated_at: u64,
    pub waiting: Option<WaitingSummary>,
    pub running: Option<RunningSummary>,
    /// Ids it ran under before a `/clear`, so a client holding one follows the re-key.
    pub previous_ids: Vec<String>,
}

#[derive(Clone, Default)]
pub struct AgentManager {
    inner: Arc<Mutex<HashMap<String, Arc<AgentSession>>>>,
    /// Held across a start's check-spawn-insert and a stop's whole shutdown, so
    /// two starts for one id can't both spawn, and a start can't slip in while
    /// a stopped process is still exiting (two writers on one session file).
    lifecycle: Arc<Mutex<()>>,
}

impl AgentManager {
    pub fn get(&self, session_id: &str) -> Option<Arc<AgentSession>> {
        lock(&self.inner).get(session_id).cloned()
    }

    /// Start a session, or return the one already running for this id.
    pub fn start(&self, req: StartAgent) -> Result<Arc<AgentSession>> {
        if !claude_transcript::is_uuid(&req.session_id) {
            bail!("session id must be a UUID");
        }
        if let Some(mode) = &req.permission_mode {
            if !PERMISSION_MODES.contains(&mode.as_str()) {
                bail!("unknown permission mode: {mode}");
            }
        }
        let _lifecycle = lock(&self.lifecycle);
        if let Some(existing) = self.get(&req.session_id) {
            return Ok(existing);
        }
        let max = std::env::var("WORKBENCH_MAX_AGENTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_AGENTS);
        if self.live_count() >= max {
            bail!("chat session limit reached ({max})");
        }

        let projects = req
            .config_dir
            .clone()
            .unwrap_or_else(workbench_core::paths::claude_user_dir)
            .join("projects");
        let history = claude_transcript::find_transcript(&projects, &req.session_id);
        let transcript = history.as_deref().map(Transcript::load).unwrap_or_default();

        let mut cmd = workbench_core::shell::command(claude_binary());
        cmd.args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--replay-user-messages",
            // Undocumented but what the Agent SDK passes: permission prompts
            // arrive as `can_use_tool` control requests on stdout.
            "--permission-prompt-tool",
            "stdio",
        ]);
        if let Some(mode) = &req.permission_mode {
            cmd.args(["--permission-mode", mode]);
        }
        let id_flag = if history.is_some() {
            "--resume"
        } else {
            "--session-id"
        };
        cmd.args([id_flag, &req.session_id]);
        cmd.current_dir(&req.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, val) in workbench_core::shell::inherited_env() {
            cmd.env(key, val);
        }
        cmd.env("PATH", workbench_core::paths::enriched_path());
        cmd.env_remove("WORKBENCH_TOKEN");
        if let Some(id) = &req.pane_id {
            cmd.env("WORKBENCH_PANE_ID", id);
        }
        if let Some(sock) = &req.hook_socket {
            cmd.env("WORKBENCH_HOOK_SOCKET", sock);
        }
        if let Some(dir) = &req.config_dir {
            cmd.env(workbench_core::claude_accounts::CONFIG_DIR_ENV, dir);
        }
        // Own process group, so stopping also ends the shells Claude started.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);

        let mut child = cmd
            .spawn()
            .context("failed to start `claude` (is the Claude CLI installed?)")?;
        let stdout = child.stdout.take().context("claude stdout")?;
        let stderr = child.stderr.take().context("claude stderr")?;
        let stdin = child.stdin.take().context("claude stdin")?;
        let pid = child.id();

        let (tx, _) = broadcast::channel(256);
        let session = Arc::new(AgentSession {
            session_id: Mutex::new(req.session_id.clone()),
            pane_id: req.pane_id,
            project_path: req.project_path,
            worktree_path: req.worktree_path,
            claude_account_id: req.claude_account_id,
            transcript: Mutex::new(transcript),
            busy_since: Mutex::new(None),
            updated_at: AtomicU64::new(now_ms()),
            tx,
            stdin: Mutex::new(Some(stdin)),
            child: Mutex::new(child),
            pid,
            exited: AtomicBool::new(false),
            task_files: Mutex::new(HashMap::new()),
        });
        // The SDK handshake: without it the CLI won't route permission prompts here.
        session.send(&json!({
            "type": "control_request",
            "request_id": uuid::Uuid::new_v4().to_string(),
            "request": {"subtype": "initialize"},
        }))?;
        lock(&self.inner).insert(req.session_id.clone(), session.clone());

        let stderr_tail = Arc::new(Mutex::new(String::new()));
        {
            let tail = stderr_tail.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    tracing::warn!("claude stderr: {line}");
                    let mut tail = lock(&tail);
                    tail.clear();
                    tail.push_str(&line);
                }
            });
        }
        let inner = self.inner.clone();
        let reader_session = session.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // The old id stays an alias: a late request for it reaches this process too.
                reader_session.apply_line(&line, |new_id| {
                    lock(&inner).insert(new_id.to_string(), reader_session.clone());
                });
            }
            reader_session.finish(&lock(&stderr_tail));
            lock(&inner).retain(|_, s| !Arc::ptr_eq(s, &reader_session));
        });
        Ok(session)
    }

    /// Stop a session's process (any of its ids). Blocking (waits out the grace period).
    pub fn stop(&self, session_id: &str) -> bool {
        let _lifecycle = lock(&self.lifecycle);
        let Some(session) = self.get(session_id) else {
            return false;
        };
        self.forget(&session);
        session.shutdown();
        true
    }

    /// Stop whatever chat session a closed pane owned. Blocking.
    pub fn stop_pane(&self, pane_id: &str) -> usize {
        let _lifecycle = lock(&self.lifecycle);
        let owned = self.sessions(|s| s.pane_id.as_deref() == Some(pane_id));
        for session in &owned {
            self.forget(session);
            session.shutdown();
        }
        owned.len()
    }

    /// Stop every session (the app is quitting or installing an update). Blocking.
    pub fn kill_all(&self) {
        let _lifecycle = lock(&self.lifecycle);
        let all = self.sessions(|_| true);
        lock(&self.inner).clear();
        let handles: Vec<_> = all
            .into_iter()
            .map(|s| std::thread::spawn(move || s.shutdown()))
            .collect();
        for handle in handles {
            let _ = handle.join();
        }
    }

    /// Every live session, most recently changed first.
    pub fn summaries(&self) -> Vec<AgentSummary> {
        let mut grouped: Vec<(Arc<AgentSession>, Vec<String>)> = Vec::new();
        for (id, session) in lock(&self.inner).iter() {
            match grouped.iter_mut().find(|(s, _)| Arc::ptr_eq(s, session)) {
                Some((_, ids)) => ids.push(id.clone()),
                None => grouped.push((session.clone(), vec![id.clone()])),
            }
        }
        let mut all: Vec<AgentSummary> = grouped
            .into_iter()
            .map(|(session, mut ids)| {
                let mut summary = session.summary();
                ids.retain(|id| *id != summary.session_id);
                ids.sort();
                summary.previous_ids = ids;
                summary
            })
            .collect();
        all.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        all
    }

    /// Distinct sessions matching `keep` — a session with aliases appears once.
    fn sessions(&self, keep: impl Fn(&AgentSession) -> bool) -> Vec<Arc<AgentSession>> {
        let mut found: Vec<Arc<AgentSession>> = lock(&self.inner)
            .values()
            .filter(|s| keep(s))
            .cloned()
            .collect();
        found.sort_by_key(|s| Arc::as_ptr(s) as usize);
        found.dedup_by(|a, b| Arc::ptr_eq(a, b));
        found
    }

    /// Drop every id (aliases included) that points at this session.
    fn forget(&self, session: &Arc<AgentSession>) {
        lock(&self.inner).retain(|_, s| !Arc::ptr_eq(s, session));
    }

    fn live_count(&self) -> usize {
        self.sessions(|_| true).len()
    }
}

impl AgentSession {
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    pub fn id(&self) -> String {
        lock(&self.session_id).clone()
    }

    pub fn summary(&self) -> AgentSummary {
        let t = lock(&self.transcript);
        let meta = t.meta();
        AgentSummary {
            session_id: self.id(),
            project_path: self.project_path.clone(),
            worktree_path: self.worktree_path.clone(),
            pane_id: self.pane_id.clone(),
            claude_account_id: self.claude_account_id.clone(),
            title: meta.title.clone(),
            model: meta.model.clone(),
            busy: meta.busy,
            exited: self.has_exited(),
            busy_since: *lock(&self.busy_since),
            updated_at: self.updated_at.load(Ordering::SeqCst),
            waiting: t.waiting_on().and_then(TranscriptItem::waiting_summary),
            running: t.running_tool().and_then(TranscriptItem::running_summary),
            previous_ids: Vec::new(),
        }
    }

    fn send(&self, msg: &Value) -> Result<()> {
        let mut stdin = lock(&self.stdin);
        let Some(pipe) = stdin.as_mut() else {
            bail!("the session has stopped");
        };
        writeln!(pipe, "{msg}").context("write to claude")?;
        pipe.flush().context("flush to claude")
    }

    fn control(&self, request: Value) -> Result<()> {
        self.send(&json!({
            "type": "control_request",
            "request_id": uuid::Uuid::new_v4().to_string(),
            "request": request,
        }))
    }

    pub fn prompt(&self, text: &str, images: &[PromptImage]) -> Result<()> {
        let content = if images.is_empty() {
            json!(text)
        } else {
            let mut blocks: Vec<Value> = images
                .iter()
                .map(|img| {
                    json!({"type": "image", "source": {
                        "type": "base64", "media_type": img.media_type, "data": img.data,
                    }})
                })
                .collect();
            if !text.trim().is_empty() {
                blocks.push(json!({"type": "text", "text": text}));
            }
            Value::Array(blocks)
        };
        self.send(&json!({
            "type": "user",
            "message": {"role": "user", "content": content},
            "parent_tool_use_id": null,
            // Hosts relaying typed input must say so; unattributed input fails
            // closed at the CLI's isHuman() trust gates.
            "origin": {"kind": "human"},
        }))?;
        let mut t = lock(&self.transcript);
        t.set_busy();
        self.broadcast_update(&t, &[]);
        Ok(())
    }

    pub fn approve(
        &self,
        request_id: &str,
        decision: ApprovalDecision,
        answers: Option<&serde_json::Map<String, Value>>,
    ) -> Result<()> {
        let mut t = lock(&self.transcript);
        let Some((i, response)) = t.resolve_approval(request_id, decision, answers) else {
            return Ok(()); // already answered (another device, or twice)
        };
        self.send(&response)?;
        self.broadcast_update(&t, &[i]);
        Ok(())
    }

    pub fn set_model(&self, model: &str) -> Result<()> {
        let valid = !model.is_empty()
            && model.len() <= 80
            && model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._[]".contains(&b));
        if !valid {
            bail!("unknown model: {model}");
        }
        self.control(json!({"subtype": "set_model", "model": model}))?;
        let mut t = lock(&self.transcript);
        t.set_model_choice(model);
        self.broadcast_update(&t, &[]);
        Ok(())
    }

    pub fn set_effort(&self, level: &str) -> Result<()> {
        if !EFFORT_LEVELS.contains(&level) {
            bail!("unknown effort level: {level}");
        }
        self.control(
            json!({"subtype": "apply_flag_settings", "settings": {"effortLevel": level}}),
        )?;
        let mut t = lock(&self.transcript);
        t.set_effort(level);
        self.broadcast_update(&t, &[]);
        Ok(())
    }

    /// The end of a background task's live output and its total size. The
    /// file's path is cached once found.
    pub fn task_output(&self, task_id: &str) -> Option<(String, u64)> {
        let known = lock(&self.task_files).get(task_id).cloned();
        let path = known.or_else(|| {
            let found = workbench_core::task_output::find(task_id)?;
            lock(&self.task_files).insert(task_id.to_string(), found.clone());
            Some(found)
        })?;
        workbench_core::task_output::tail(&path, TASK_OUTPUT_TAIL).ok()
    }

    /// The whole output of a tool whose chat item carries a preview.
    pub fn full_output(&self, tool_id: &str) -> Option<String> {
        lock(&self.transcript)
            .full_output(tool_id)
            .map(String::from)
    }

    pub fn interrupt(&self) -> Result<()> {
        self.control(json!({"subtype": "interrupt"}))
    }

    pub fn set_mode(&self, mode: &str) -> Result<()> {
        if !PERMISSION_MODES.contains(&mode) {
            bail!("unknown permission mode: {mode}");
        }
        self.control(json!({"subtype": "set_permission_mode", "mode": mode}))?;
        let mut t = lock(&self.transcript);
        t.set_permission_mode(mode);
        self.broadcast_update(&t, &[]);
        Ok(())
    }

    /// The attach snapshot and a receiver for every later frame, taken under
    /// the transcript lock so no update falls between them.
    pub fn subscribe(&self) -> (String, broadcast::Receiver<String>) {
        let t = lock(&self.transcript);
        let rx = self.tx.subscribe();
        (self.snapshot(&t), rx)
    }

    fn snapshot(&self, t: &Transcript) -> String {
        let items = t.items();
        let start = items.len().saturating_sub(SNAPSHOT_ITEMS);
        let exited = self.has_exited();
        json!({
            "t": "snapshot",
            "sessionId": self.id(),
            "start": start,
            "items": &items[start..],
            "meta": t.meta(),
            "commands": t.commands(),
            "exited": exited,
        })
        .to_string()
    }

    /// Apply one stdout line. `alias` registers the new id when `/clear` moves
    /// the conversation — before any client hears of it, so a start or attach
    /// with the new id can never spawn a second process.
    fn apply_line(&self, line: &str, alias: impl FnOnce(&str)) {
        let mut t = lock(&self.transcript);
        let applied = t.apply_line(line);
        if let Some(kind) = &applied.unknown_kind {
            tracing::warn!("claude sent an unrecognised message kind: {kind}");
        }
        if let Some(reply) = &applied.reply {
            if let Err(e) = self.send(reply) {
                tracing::warn!("could not answer a claude control request: {e}");
            }
        }
        if applied.commands {
            let frame = json!({"t": "commands", "commands": t.commands()});
            let _ = self.tx.send(frame.to_string());
        }
        if let Some(new_id) = applied.new_session_id {
            *lock(&self.session_id) = new_id.clone();
            alias(&new_id);
            self.touch(&t);
            let _ = self.tx.send(self.snapshot(&t));
            return;
        }
        if !applied.items.is_empty() || applied.meta {
            self.broadcast_update(&t, &applied.items);
        }
    }

    /// Frames go out while the transcript lock is held, so their order matches
    /// the order changes were applied in.
    fn broadcast_update(&self, t: &Transcript, changed: &[usize]) {
        self.touch(t);
        let mut indices = changed.to_vec();
        indices.sort_unstable();
        indices.dedup();
        let changes: Vec<Value> = indices.iter().map(|&i| json!([i, &t.items()[i]])).collect();
        let frame = json!({"t": "update", "changes": changes, "meta": t.meta()});
        let _ = self.tx.send(frame.to_string());
    }

    fn touch(&self, t: &Transcript) {
        let now = now_ms();
        self.updated_at.store(now, Ordering::SeqCst);
        let mut since = lock(&self.busy_since);
        *since = t.meta().busy.then(|| since.unwrap_or(now));
    }

    fn finish(&self, stderr_tail: &str) {
        lock(&self.stdin).take();
        // Until the leader is reaped below its pid still names its process
        // group: end the background shells Claude started, which would
        // otherwise outlive it holding ports and files.
        #[cfg(unix)]
        unsafe {
            libc::killpg(self.pid as libc::pid_t, libc::SIGTERM);
        }
        let code = lock(&self.child).wait().ok().and_then(|s| s.code());
        self.exited.store(true, Ordering::SeqCst);
        let message = (code != Some(0) && !stderr_tail.is_empty()).then_some(stderr_tail);
        let _ = self
            .tx
            .send(json!({"t": "exit", "code": code, "message": message}).to_string());
    }

    /// Interrupt, close stdin, and kill the process (and its group) if it
    /// lingers. The reader thread's `finish` reaps it and ends the group.
    fn shutdown(&self) {
        let _ = self.interrupt();
        // Windows has no process groups: `taskkill /T` walks the tree, which it
        // can only do while Claude is still alive to be its root.
        #[cfg(windows)]
        {
            std::thread::sleep(Duration::from_millis(500));
            let _ = workbench_core::shell::command("taskkill")
                .args(["/T", "/F", "/PID", &self.pid.to_string()])
                .output();
        }
        lock(&self.stdin).take();
        let deadline = Instant::now() + STOP_GRACE;
        while !self.exited.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        if !self.exited.load(Ordering::SeqCst) {
            #[cfg(unix)]
            unsafe {
                libc::killpg(self.pid as libc::pid_t, libc::SIGKILL);
            }
            #[cfg(windows)]
            let _ = lock(&self.child).kill();
        }
    }
}

/// `WORKBENCH_CLAUDE_BIN`, else `claude` found on the enriched search path
/// (GUI apps don't inherit the shell's PATH).
fn claude_binary() -> PathBuf {
    if let Some(bin) = std::env::var_os("WORKBENCH_CLAUDE_BIN") {
        return bin.into();
    }
    let name = if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    };
    std::env::split_paths(&workbench_core::paths::enriched_path())
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| name.into())
}
