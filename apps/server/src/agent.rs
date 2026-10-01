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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use tokio::sync::broadcast;
use workbench_core::claude_transcript::{self, ApprovalDecision, Transcript};

/// The modes `--permission-mode` / `set_permission_mode` accept.
pub const PERMISSION_MODES: &[&str] = &[
    "default",
    "acceptEdits",
    "plan",
    "auto",
    "dontAsk",
    "bypassPermissions",
];

const DEFAULT_MAX_AGENTS: usize = 16;
/// Items in an attach snapshot; older history stays on disk.
const SNAPSHOT_ITEMS: usize = 500;
const STOP_GRACE: Duration = Duration::from_secs(3);
/// How much of a background task's output the panel shows.
const TASK_OUTPUT_TAIL: u64 = 64 * 1024;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// An image pasted into the chat, base64-encoded.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptImage {
    pub media_type: String,
    pub data: String,
}

/// Formats the Claude API accepts.
const IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
pub const MAX_IMAGES: usize = 10;
/// The API's per-image cap is 5 MB decoded; base64 is 4/3 of that.
const MAX_IMAGE_BASE64: usize = 5 * 1024 * 1024 * 4 / 3 + 4;

impl PromptImage {
    pub fn validate(&self) -> Result<()> {
        if !IMAGE_TYPES.contains(&self.media_type.as_str()) {
            bail!("unsupported image type: {}", self.media_type);
        }
        if self.data.len() > MAX_IMAGE_BASE64 {
            bail!("images must be under 5 MB");
        }
        if !self
            .data
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
        {
            bail!("image data must be base64");
        }
        Ok(())
    }
}

pub struct StartAgent {
    pub cwd: String,
    pub session_id: String,
    pub permission_mode: Option<String>,
    /// Forwarded as `WORKBENCH_PANE_ID` / `WORKBENCH_HOOK_SOCKET` so hooks keep
    /// driving the desktop's activity tracking, as for terminal panes.
    pub pane_id: Option<String>,
    pub hook_socket: Option<String>,
}

pub struct AgentSession {
    /// Changes when `/clear` continues the conversation under a new id.
    session_id: Mutex<String>,
    pub pane_id: Option<String>,
    transcript: Mutex<Transcript>,
    tx: broadcast::Sender<String>,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Child>,
    task_files: Mutex<HashMap<String, PathBuf>>,
}

#[derive(Clone, Default)]
pub struct AgentManager {
    inner: Arc<Mutex<HashMap<String, Arc<AgentSession>>>>,
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
        if let Some(existing) = self.get(&req.session_id) {
            return Ok(existing);
        }
        let max = std::env::var("WORKBENCH_MAX_AGENTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_AGENTS);
        if lock(&self.inner).len() >= max {
            bail!("chat session limit reached ({max})");
        }

        let projects = workbench_core::paths::claude_user_dir().join("projects");
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
        cmd.env("PATH", search_path());
        cmd.env_remove("WORKBENCH_TOKEN");
        if let Some(id) = &req.pane_id {
            cmd.env("WORKBENCH_PANE_ID", id);
        }
        if let Some(sock) = &req.hook_socket {
            cmd.env("WORKBENCH_HOOK_SOCKET", sock);
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

        let (tx, _) = broadcast::channel(256);
        let session = Arc::new(AgentSession {
            session_id: Mutex::new(req.session_id.clone()),
            pane_id: req.pane_id,
            transcript: Mutex::new(transcript),
            tx,
            stdin: Mutex::new(Some(stdin)),
            child: Mutex::new(child),
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
                if let Some((old, new)) = reader_session.apply_line(&line) {
                    let mut map = lock(&inner);
                    if let Some(s) = map.remove(&old) {
                        map.insert(new, s);
                    }
                }
            }
            reader_session.finish(&lock(&stderr_tail));
            let mut map = lock(&inner);
            let id = reader_session.id();
            if map
                .get(&id)
                .is_some_and(|s| Arc::ptr_eq(s, &reader_session))
            {
                map.remove(&id);
            }
        });
        Ok(session)
    }

    /// Stop a session's process. Blocking (waits out the grace period).
    pub fn stop(&self, session_id: &str) -> bool {
        let Some(session) = lock(&self.inner).remove(session_id) else {
            return false;
        };
        session.shutdown();
        true
    }

    /// Stop whatever chat session a closed pane owned. Blocking.
    pub fn stop_pane(&self, pane_id: &str) -> usize {
        let ids: Vec<String> = lock(&self.inner)
            .values()
            .filter(|s| s.pane_id.as_deref() == Some(pane_id))
            .map(|s| s.id())
            .collect();
        ids.iter().filter(|id| self.stop(id)).count()
    }
}

impl AgentSession {
    pub fn id(&self) -> String {
        lock(&self.session_id).clone()
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
        let exited = lock(&self.stdin).is_none();
        json!({
            "t": "snapshot",
            "sessionId": self.id(),
            "start": start,
            "items": &items[start..],
            "meta": t.meta(),
            "exited": exited,
        })
        .to_string()
    }

    /// Apply one stdout line. Returns `(old, new)` ids when `/clear` moved the
    /// conversation to a new session id.
    fn apply_line(&self, line: &str) -> Option<(String, String)> {
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
        if let Some(new_id) = applied.new_session_id {
            let old = std::mem::replace(&mut *lock(&self.session_id), new_id.clone());
            let _ = self.tx.send(self.snapshot(&t));
            return Some((old, new_id));
        }
        if !applied.items.is_empty() || applied.meta {
            self.broadcast_update(&t, &applied.items);
        }
        None
    }

    /// Frames go out while the transcript lock is held, so their order matches
    /// the order changes were applied in.
    fn broadcast_update(&self, t: &Transcript, changed: &[usize]) {
        let mut indices = changed.to_vec();
        indices.sort_unstable();
        indices.dedup();
        let changes: Vec<Value> = indices.iter().map(|&i| json!([i, &t.items()[i]])).collect();
        let frame = json!({"t": "update", "changes": changes, "meta": t.meta()});
        let _ = self.tx.send(frame.to_string());
    }

    fn finish(&self, stderr_tail: &str) {
        lock(&self.stdin).take();
        let code = lock(&self.child).wait().ok().and_then(|s| s.code());
        let message = (code != Some(0) && !stderr_tail.is_empty()).then_some(stderr_tail);
        let _ = self
            .tx
            .send(json!({"t": "exit", "code": code, "message": message}).to_string());
    }

    /// Interrupt, close stdin, then end the whole process group if it lingers.
    fn shutdown(&self) {
        let _ = self.interrupt();
        lock(&self.stdin).take();
        let deadline = Instant::now() + STOP_GRACE;
        while Instant::now() < deadline {
            if matches!(lock(&self.child).try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let pid = lock(&self.child).id();
        #[cfg(unix)]
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGKILL);
        }
        #[cfg(windows)]
        {
            let _ = workbench_core::shell::command("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .output();
        }
        let _ = lock(&self.child).wait();
    }
}

/// `WORKBENCH_CLAUDE_BIN`, else `claude` found on the search path. GUI apps on
/// macOS don't inherit the shell's PATH, and the native installer puts the CLI
/// in `~/.local/bin`, so that is searched too.
fn claude_binary() -> PathBuf {
    if let Some(bin) = std::env::var_os("WORKBENCH_CLAUDE_BIN") {
        return bin.into();
    }
    let name = if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    };
    std::env::split_paths(&search_path())
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| name.into())
}

fn search_path() -> std::ffi::OsString {
    let local_bin = workbench_core::paths::home_dir().join(".local").join("bin");
    let enriched = workbench_core::paths::enriched_path();
    let dirs = std::iter::once(local_bin).chain(std::env::split_paths(&enriched));
    std::env::join_paths(dirs).unwrap_or(enriched)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(media_type: &str, data: &str) -> PromptImage {
        PromptImage {
            media_type: media_type.into(),
            data: data.into(),
        }
    }

    #[test]
    fn prompt_images_must_be_api_formats_in_base64() {
        assert!(image("image/png", "iVBORw==").validate().is_ok());
        assert!(image("image/svg+xml", "PHN2Zz4=").validate().is_err());
        assert!(image("image/png", "not base64!").validate().is_err());
        assert!(image("image/png", &"A".repeat(MAX_IMAGE_BASE64 + 1))
            .validate()
            .is_err());
    }
}
