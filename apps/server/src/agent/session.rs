//! One chat session's process and its clients: spawning, writing to its
//! stdin, folding its stdout through the [`Driver`], and broadcasting every
//! change to attached clients (desktop chat pane, phone) as `update` frames.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use tokio::sync::broadcast;
use workbench_core::claude_transcript::{
    ApprovalDecision, ChatView, ElicitationAction, TranscriptItem, TranscriptMeta, KEEPALIVE_PROMPT,
};

use super::cache::{self, CachePolicy, PolicyStore, Upkeep};
use super::driver::{Driver, Effects, Launch};
use super::{lock, now_ms, AgentKind, AgentSummary, PromptFile, PromptImage, StartAgent};

/// Items in an attach snapshot; older history stays on disk.
pub(super) const SNAPSHOT_ITEMS: usize = 500;
const STOP_GRACE: Duration = Duration::from_secs(3);
/// How much of a background task's output the panel shows.
const TASK_OUTPUT_TAIL: u64 = 64 * 1024;
/// How long a host request (a file rewind) may take the CLI to answer.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);

type Waiter = std::sync::mpsc::Sender<Result<Value, String>>;

pub(super) type Registry = Arc<Mutex<HashMap<String, Arc<AgentSession>>>>;

/// The registry key of a new Codex thread until codex gives it an id: it is
/// registered from the start so stops, pane closes and the cap all see it.
const PENDING: &str = "pending:";

pub(super) fn is_pending(key: &str) -> bool {
    key.starts_with(PENDING)
}

pub struct AgentSession {
    pub kind: AgentKind,
    /// Changes when `/clear` continues the conversation under a new id; empty
    /// until a new Codex thread has one.
    session_id: Mutex<String>,
    pub pane_id: Option<String>,
    project_path: String,
    worktree_path: Option<String>,
    claude_account_id: Option<String>,
    driver: Mutex<Driver>,
    /// Unix ms; both are written under the driver lock.
    busy_since: Mutex<Option<u64>>,
    updated_at: AtomicU64,
    /// When the last turn went idle, so a poller catches turns shorter than its interval.
    turn_ended_at: Mutex<Option<u64>>,
    tx: broadcast::Sender<String>,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Child>,
    /// Kept apart from `child` so a stop never waits on the reaping lock.
    pid: u32,
    exited: AtomicBool,
    task_files: Mutex<HashMap<String, PathBuf>>,
    stderr_tail: Mutex<String>,
    program: &'static str,
    /// Set once the session has an id clients can use, or failed to get one.
    ready: Mutex<Option<Result<String, String>>>,
    ready_cv: Condvar,
    /// How it was started, to start it again resumed elsewhere (a rewind).
    relaunch: StartAgent,
    /// Host requests awaiting the CLI's answer, by request id.
    waiters: Mutex<HashMap<String, Waiter>>,
    /// Stopped to make way for a relaunch: clients re-attach, not end.
    replaced: AtomicBool,
    /// Ended on purpose (End session), not by a crash, `/exit` or a handoff.
    ended: AtomicBool,
    cache_policy: Mutex<CachePolicy>,
    /// Where policies are saved, by session id.
    cache_policies: Arc<PolicyStore>,
    /// The cache expiry upkeep last acted on, so it acts once per expiry.
    upkept_for: Mutex<Option<u64>>,
}

/// The command every chat process starts from: cwd, pipes, the inherited
/// environment without the server's token, the pane's hook wiring, and its
/// own process group so stopping it also ends the shells it started.
pub(super) fn base_command(program: impl AsRef<std::ffi::OsStr>, req: &StartAgent) -> Command {
    let mut cmd = workbench_core::shell::command(program);
    cmd.current_dir(&req.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, val) in workbench_core::shell::inherited_env() {
        cmd.env(key, val);
    }
    cmd.env("PATH", workbench_core::paths::enriched_path());
    cmd.env_remove("WORKBENCH_TOKEN");
    // The Workbench plugin (Claude) and the notify bridge (Codex) keep driving the desktop's
    // activity tracking, as for terminal panes.
    if let Some(id) = &req.pane_id {
        cmd.env("WORKBENCH_PANE_ID", id);
    }
    if let Some(sock) = &req.hook_socket {
        cmd.env("WORKBENCH_HOOK_SOCKET", sock);
        if let Some(dirs) = workbench_core::claude_plugin::plugin_dirs_env() {
            cmd.env(workbench_core::claude_plugin::PLUGIN_DIRS_ENV, dirs);
        }
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd
}

impl AgentSession {
    /// Start the process and its reader threads, registered under its id (or a
    /// pending key) before the reader can see it exit. `registry` is also
    /// where aliases go when the id changes.
    pub(super) fn spawn(
        req: StartAgent,
        launch: Launch,
        registry: Registry,
        cache_policies: Arc<PolicyStore>,
    ) -> Result<Arc<Self>> {
        let relaunch = req.clone();
        let Launch {
            mut cmd,
            driver,
            hello,
            ready,
            program,
        } = launch;
        let mut child = cmd.spawn().with_context(|| {
            format!("failed to start `{program}` (is the {program} CLI installed?)")
        })?;
        let stdout = child.stdout.take().context("stdout")?;
        let stderr = child.stderr.take().context("stderr")?;
        let stdin = child.stdin.take().context("stdin")?;
        let pid = child.id();
        let known_id = req.launch.known_id().map(String::from);
        let kind = req.launch.kind();
        let cache_policy = match &known_id {
            Some(id) if kind == AgentKind::Claude => cache_policies.get(id),
            _ => CachePolicy::default(),
        };

        let (tx, _) = broadcast::channel(256);
        let session = Arc::new(Self {
            kind,
            session_id: Mutex::new(known_id.clone().unwrap_or_default()),
            pane_id: req.pane_id,
            project_path: req.project_path,
            worktree_path: req.worktree_path,
            claude_account_id: req.claude_account_id,
            driver: Mutex::new(driver),
            busy_since: Mutex::new(None),
            updated_at: AtomicU64::new(now_ms()),
            turn_ended_at: Mutex::new(None),
            tx,
            stdin: Mutex::new(Some(stdin)),
            child: Mutex::new(child),
            pid,
            exited: AtomicBool::new(false),
            task_files: Mutex::new(HashMap::new()),
            stderr_tail: Mutex::new(String::new()),
            program,
            ready: Mutex::new(ready.map(Ok)),
            ready_cv: Condvar::new(),
            relaunch,
            waiters: Mutex::new(HashMap::new()),
            replaced: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            cache_policy: Mutex::new(cache_policy),
            cache_policies,
            upkept_for: Mutex::new(None),
        });
        let key = known_id.unwrap_or_else(|| format!("{PENDING}{}", uuid::Uuid::new_v4()));
        lock(&registry).insert(key, session.clone());
        if let Err(e) = hello.iter().try_for_each(|line| session.send(line)) {
            lock(&registry).retain(|_, s| !Arc::ptr_eq(s, &session));
            // No reader thread yet to reap it.
            let mut child = lock(&session.child);
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }

        let reader = session.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                tracing::warn!("{} stderr: {line}", reader.program);
                *lock(&reader.stderr_tail) = strip_ansi(&line);
            }
        });
        let reader = session.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // The old id stays an alias: a late request for it reaches this process too.
                reader.apply_line(&line, |new_id| {
                    lock(&registry).insert(new_id.to_string(), reader.clone());
                });
            }
            reader.finish();
            lock(&registry).retain(|_, s| !Arc::ptr_eq(s, &reader));
        });
        Ok(session)
    }

    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    pub fn id(&self) -> String {
        lock(&self.session_id).clone()
    }

    /// Block until the session has its id (a Codex thread starting or
    /// resuming), the process failed, or `timeout` passed.
    pub(super) fn wait_ready(&self, timeout: Duration) -> Result<String> {
        let guard = lock(&self.ready);
        let (guard, _) = self
            .ready_cv
            .wait_timeout_while(guard, timeout, |r| r.is_none())
            .unwrap_or_else(|e| e.into_inner());
        match &*guard {
            Some(Ok(id)) => Ok(id.clone()),
            Some(Err(e)) => bail!("{e}"),
            None => bail!(
                "`{}` didn't start within {}s{}",
                self.program,
                timeout.as_secs(),
                self.stderr_suffix()
            ),
        }
    }

    fn set_ready(&self, result: Result<String, String>) {
        let mut ready = lock(&self.ready);
        if ready.is_none() {
            if let Ok(id) = &result {
                *lock(&self.session_id) = id.clone();
            }
            *ready = Some(result);
            self.ready_cv.notify_all();
        }
    }

    fn stderr_suffix(&self) -> String {
        let tail = lock(&self.stderr_tail);
        if tail.is_empty() {
            String::new()
        } else {
            format!(": {tail}")
        }
    }

    pub fn summary(&self) -> AgentSummary {
        let d = lock(&self.driver);
        let view = d.view();
        let meta = view.meta();
        AgentSummary {
            agent: self.kind,
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
            turn_ended_at: *lock(&self.turn_ended_at),
            waiting: view.waiting_on().and_then(TranscriptItem::waiting_summary),
            running: view
                .running_tool()
                .and_then(TranscriptItem::running_summary),
            previous_ids: Vec::new(),
        }
    }

    fn send(&self, msg: &Value) -> Result<()> {
        let mut stdin = lock(&self.stdin);
        let Some(pipe) = stdin.as_mut() else {
            bail!("the session has stopped");
        };
        writeln!(pipe, "{msg}").with_context(|| format!("write to {}", self.program))?;
        pipe.flush()
            .with_context(|| format!("flush to {}", self.program))
    }

    /// Apply a client message through the driver. Its lines are written
    /// outside the driver lock: a prompt full of images must not stall the
    /// reader thread (and with it the process's stdout).
    fn run(&self, op: impl FnOnce(&mut Driver) -> Result<Effects>) -> Result<()> {
        let effects = op(&mut lock(&self.driver))?;
        for msg in &effects.send {
            self.send(msg)?;
        }
        if effects.meta || !effects.items.is_empty() {
            let d = lock(&self.driver);
            self.broadcast_update(d.view(), &effects.items);
        }
        Ok(())
    }

    pub fn prompt(&self, text: &str, images: &[PromptImage], files: &[PromptFile]) -> Result<()> {
        self.run(|d| d.prompt(text, images, files))
    }

    pub fn approve(
        &self,
        request_id: &str,
        decision: ApprovalDecision,
        answers: Option<&Map<String, Value>>,
    ) -> Result<()> {
        self.run(|d| d.approve(request_id, decision, answers))
    }

    pub fn elicit(
        &self,
        request_id: &str,
        action: ElicitationAction,
        content: Option<&Map<String, Value>>,
    ) -> Result<()> {
        self.run(|d| Ok(d.elicit(request_id, action, content)))
    }

    pub fn interrupt(&self) -> Result<()> {
        self.run(Driver::interrupt)
    }

    pub fn set_mode(&self, mode: &str) -> Result<()> {
        self.run(|d| d.set_mode(mode))
    }

    pub fn set_model(&self, model: &str) -> Result<()> {
        self.run(|d| d.set_model(model))
    }

    pub fn set_effort(&self, level: &str) -> Result<()> {
        self.run(|d| d.set_effort(level))
    }

    pub fn cache_policy(&self) -> CachePolicy {
        lock(&self.cache_policy).clone()
    }

    /// Replace and save the cache policy, and tell every attached client.
    pub(super) fn set_cache_policy(&self, policy: CachePolicy) -> Result<()> {
        if self.kind != AgentKind::Claude {
            bail!("Only Claude chats have a prompt cache to manage.");
        }
        *lock(&self.cache_policy) = policy.clone();
        self.cache_policies.set(&self.id(), &policy);
        let _ = self
            .tx
            .send(json!({"t": "cachePolicy", "policy": policy}).to_string());
        Ok(())
    }

    /// Refresh the prompt cache now with a hidden keep-alive turn.
    pub fn keep_cache_warm(&self) -> Result<()> {
        if self.kind != AgentKind::Claude {
            bail!("Only Claude chats have a prompt cache to manage.");
        }
        if !self.is_idle() {
            bail!("Claude is mid-turn, which keeps the cache warm anyway.");
        }
        self.prompt(KEEPALIVE_PROMPT, &[], &[])
    }

    fn is_idle(&self) -> bool {
        let d = lock(&self.driver);
        !d.view().meta().busy && d.view().waiting_on().is_none()
    }

    /// Run whatever the cache policy calls for at `now`, once per expiry.
    pub(super) fn upkeep(&self, now: u64) {
        if self.kind != AgentKind::Claude || self.has_exited() {
            return;
        }
        let policy = self.cache_policy();
        let (due, expires) = {
            let d = lock(&self.driver);
            let view = d.view();
            let meta = view.meta();
            let idle = !meta.busy && view.waiting_on().is_none();
            (cache::due(&policy, meta, idle, now), meta.cache_expires_at)
        };
        let Some(due) = due else {
            return;
        };
        if std::mem::replace(&mut *lock(&self.upkept_for), expires) == expires {
            return;
        }
        let text = match due {
            Upkeep::KeepAlive => KEEPALIVE_PROMPT,
            Upkeep::Compact => "/compact",
        };
        if let Err(e) = self.prompt(text, &[], &[]) {
            tracing::warn!("cache upkeep ({due:?}) failed: {e}");
        }
    }

    /// The session's meta, or an error while a turn runs: a rewind then
    /// would race the edits and messages still being made.
    pub(super) fn idle_meta(&self) -> Result<TranscriptMeta> {
        let d = lock(&self.driver);
        let meta = d.view().meta().clone();
        if meta.busy || d.view().waiting_on().is_some() {
            bail!("Wait for the current turn to finish, or stop it, before rewinding.");
        }
        Ok(meta)
    }

    /// Restore the files Claude changed since the prompt `message_id`, or
    /// with `dry_run` only report what would change (`RewindFilesResult`).
    pub fn rewind_files(&self, message_id: &str, dry_run: bool) -> Result<Value> {
        self.idle_meta()?;
        let (tx, rx) = std::sync::mpsc::channel();
        let (request_id, effects) = lock(&self.driver).rewind_files(message_id, dry_run)?;
        lock(&self.waiters).insert(request_id.clone(), tx);
        let sent = effects.send.iter().try_for_each(|msg| self.send(msg));
        let reply = sent.and_then(|()| {
            rx.recv_timeout(CONTROL_TIMEOUT)
                .map_err(|_| anyhow::anyhow!("{} didn't answer the rewind", self.program))
        });
        lock(&self.waiters).remove(&request_id);
        reply?.map_err(anyhow::Error::msg)
    }

    /// The start request to launch this conversation again, under its current id.
    pub(super) fn relaunch(&self) -> StartAgent {
        let mut req = self.relaunch.clone();
        if let super::Launch::Claude { session_id, .. } = &mut req.launch {
            *session_id = self.id();
        }
        req
    }

    /// Stop the process for a relaunch under the same id.
    pub(super) fn replace(&self) {
        self.replaced.store(true, Ordering::SeqCst);
        self.shutdown();
    }

    /// Stop the process because the person ended the chat; its exit frame says so.
    pub(super) fn end(&self) {
        self.ended.store(true, Ordering::SeqCst);
        self.shutdown();
    }

    /// The end of a background task's live output and its total size. The
    /// file's path is cached once found. Claude only.
    pub fn task_output(&self, task_id: &str) -> Option<(String, u64)> {
        if self.kind != AgentKind::Claude {
            return None;
        }
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
        lock(&self.driver)
            .view()
            .full_output(tool_id)
            .map(String::from)
    }

    /// The attach snapshot and a receiver for every later frame, taken under
    /// the driver lock so no update falls between them.
    pub fn subscribe(&self) -> (String, broadcast::Receiver<String>) {
        let d = lock(&self.driver);
        let rx = self.tx.subscribe();
        (self.snapshot(d.view()), rx)
    }

    fn snapshot(&self, t: &dyn ChatView) -> String {
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
            "cachePolicy": self.cache_policy(),
            "exited": exited,
        })
        .to_string()
    }

    /// Apply one stdout line. `alias` registers the new id when `/clear` moves
    /// the conversation — before any client hears of it, so a start or attach
    /// with the new id can never spawn a second process.
    fn apply_line(&self, line: &str, alias: impl FnOnce(&str)) {
        let mut d = lock(&self.driver);
        let effects = d.apply_line(line);
        if let Some((id, reply)) = effects.response {
            if let Some(waiter) = lock(&self.waiters).remove(&id) {
                let _ = waiter.send(reply);
            }
        }
        for msg in &effects.send {
            if let Err(e) = self.send(msg) {
                tracing::warn!("could not answer {}: {e}", self.program);
            }
        }
        let view = d.view();
        if effects.commands {
            let frame = json!({"t": "commands", "commands": view.commands()});
            let _ = self.tx.send(frame.to_string());
        }
        if let Some(new_id) = effects.new_id {
            *lock(&self.session_id) = new_id.clone();
            // The policy follows the conversation to its new id.
            let policy = self.cache_policy();
            if policy != CachePolicy::default() {
                self.cache_policies.set(&new_id, &policy);
            }
            alias(&new_id);
            self.touch(view);
            let _ = self.tx.send(self.snapshot(view));
            return;
        }
        if let Some(ready) = effects.ready {
            self.set_ready(ready);
        }
        if effects.snapshot {
            self.touch(view);
            let _ = self.tx.send(self.snapshot(view));
        } else if !effects.items.is_empty() || effects.meta {
            self.broadcast_update(view, &effects.items);
        }
    }

    /// Frames go out while the driver lock is held, so their order matches
    /// the order changes were applied in.
    fn broadcast_update(&self, t: &dyn ChatView, changed: &[usize]) {
        self.touch(t);
        let mut indices = changed.to_vec();
        indices.sort_unstable();
        indices.dedup();
        let changes: Vec<Value> = indices.iter().map(|&i| json!([i, &t.items()[i]])).collect();
        let frame = json!({"t": "update", "changes": changes, "meta": t.meta()});
        let _ = self.tx.send(frame.to_string());
    }

    fn touch(&self, t: &dyn ChatView) {
        let now = now_ms();
        self.updated_at.store(now, Ordering::SeqCst);
        let busy = t.meta().busy;
        let mut since = lock(&self.busy_since);
        if since.is_some() && !busy {
            *lock(&self.turn_ended_at) = Some(now);
        }
        *since = busy.then(|| since.unwrap_or(now));
    }

    fn finish(&self) {
        lock(&self.stdin).take();
        // Until the leader is reaped below its pid still names its process
        // group: end the background shells it started, which would otherwise
        // outlive it holding ports and files.
        #[cfg(unix)]
        unsafe {
            libc::killpg(self.pid as libc::pid_t, libc::SIGTERM);
        }
        let code = lock(&self.child).wait().ok().and_then(|s| s.code());
        self.exited.store(true, Ordering::SeqCst);
        let tail = lock(&self.stderr_tail).clone();
        self.set_ready(Err(format!(
            "`{}` exited before it was ready{}",
            self.program,
            self.stderr_suffix()
        )));
        let frame = if self.replaced.load(Ordering::SeqCst) {
            json!({"t": "replaced"})
        } else {
            let message = (code != Some(0) && !tail.is_empty()).then_some(tail);
            let ended = self.ended.load(Ordering::SeqCst);
            json!({"t": "exit", "code": code, "message": message, "ended": ended})
        };
        let _ = self.tx.send(frame.to_string());
    }

    /// Interrupt, close stdin, and kill the process (and its group) if it
    /// lingers. The reader thread's `finish` reaps it and ends the group.
    pub(super) fn shutdown(&self) {
        let _ = self.interrupt();
        // Windows has no process groups: `taskkill /T` walks the tree, which it
        // can only do while the process is still alive to be its root.
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

/// Codex colours its log lines.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
