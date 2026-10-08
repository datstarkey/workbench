//! One chat session and its clients: a `codex app-server` spawned and spoken
//! to over its stdio, or a terminal `claude` the Workbench plugin feeds
//! (`modlink`). Lines fold through the [`Driver`], and every change goes to
//! attached clients (desktop chat pane, phone) as `update` frames.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use tokio::sync::broadcast;
use workbench_core::claude_transcript::{
    ApprovalDecision, ChatView, ElicitationAction, TranscriptItem, TranscriptMeta, WaitingSummary,
    KEEPALIVE_PROMPT,
};

use super::cache::{self, CachePolicy, PolicyStore, Upkeep};
use super::driver::{Driver, Effects, Launch};
use super::frames::{Frame, Frames};
use super::modlink::ModLink;
use super::{lock, now_ms, AgentKind, AgentSummary, PromptFile, PromptImage, StartAgent};

/// Items in an attach snapshot; older history stays on disk.
pub(super) const SNAPSHOT_ITEMS: usize = 500;
const STOP_GRACE: Duration = Duration::from_secs(3);
/// How much of a background task's output the panel shows.
const TASK_OUTPUT_TAIL: u64 = 64 * 1024;
/// `assistant` lines per session id that may read the cache's lifetime
/// from its transcript until it names one.
const EARLY_TTL_READS: u8 = 8;
/// Waits between reads of a `goal_status` row the CLI hadn't written yet.
const GOAL_RETRY_MS: [u64; 5] = [100, 250, 500, 1000, 2000];

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
    previous_ids: Mutex<Vec<String>>,
    attention: crate::attention_feed::AttentionFeed,
    attention_source: u64,
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
    /// The running (or next) turn is a cache keep-alive, so its end isn't recorded.
    keepalive_turn: AtomicBool,
    frames: Frames,
    /// Held by whatever changes the driver (lines, client messages) across
    /// work done outside the driver lock, so readers (the session list, an
    /// attach) never wait on file IO while changes still apply in order.
    writer: Mutex<()>,
    stdin: Mutex<Option<ChildStdin>>,
    outgoing: Mutex<Option<mpsc::SyncSender<String>>>,
    outgoing_bytes: AtomicUsize,
    /// `None` for a session fed by a terminal's plugin: there is no process of ours.
    child: Mutex<Option<Child>>,
    /// Kept apart from `child` so a stop never waits on the reaping lock.
    pid: Option<u32>,
    link: Option<Arc<ModLink>>,
    exited: AtomicBool,
    task_files: Mutex<HashMap<String, PathBuf>>,
    stderr_tail: Mutex<String>,
    /// The CLI, for messages: `codex`, or `claude` in a terminal.
    program: &'static str,
    /// Set once the session has an id clients can use, or failed to get one.
    ready: Mutex<Option<Result<String, String>>>,
    ready_cv: Condvar,
    /// How it was started, to start it again resumed elsewhere (a rewind).
    relaunch: StartAgent,
    /// Stopped to make way for a relaunch: clients re-attach, not end.
    replaced: AtomicBool,
    /// Ended on purpose (End session), not by a crash, `/exit` or a handoff.
    ended: AtomicBool,
    cache_policy: Mutex<CachePolicy>,
    /// Where policies are saved, by session id.
    cache_policies: Arc<PolicyStore>,
    /// The cache expiry upkeep last acted on, so it acts once per expiry.
    upkept_for: Mutex<Option<u64>>,
    /// The session JSONL, found once per session id (`learn_cache_ttl`).
    transcript_path: Mutex<Option<(String, PathBuf)>>,
    /// Early reads of the cache's lifetime left, for the id they're for
    /// (`learn_cache_ttl_early`).
    early_ttl_reads: Mutex<(String, u8)>,
}

/// The command a chat process starts from: cwd, pipes, the inherited
/// environment without the server's token, the pane's hook wiring, and its
/// own process group so stopping it also ends the shells it started.
pub(super) fn base_command(program: impl AsRef<std::ffi::OsStr>, req: &StartAgent) -> Command {
    // `tool` sets the enriched PATH, and only when it found the program: the
    // inherited PATH must not override it (a bare name with PATH set forks).
    let mut cmd = workbench_core::shell::tool(program);
    cmd.current_dir(&req.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, val) in workbench_core::shell::inherited_env() {
        if key != "PATH" {
            cmd.env(key, val);
        }
    }
    cmd.env_remove("WORKBENCH_TOKEN");
    // The notify bridge keeps driving the desktop's activity tracking, as for terminal panes.
    if let Some(id) = &req.pane_id {
        cmd.env("WORKBENCH_PANE_ID", id);
    }
    if let Some(sock) = &req.hook_socket {
        cmd.env("WORKBENCH_HOOK_SOCKET", sock);
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd
}

/// Register `session` under `new_id`; a resumed conversation drops its old ids.
pub(super) fn rekey(registry: &Registry, session: &Arc<AgentSession>, new_id: &str, resumed: bool) {
    let mut registry = lock(registry);
    if resumed {
        registry.retain(|_, s| !Arc::ptr_eq(s, session));
    }
    registry.insert(new_id.to_string(), session.clone());
    drop(registry);
    session.attention.sessions_changed();
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
        attention: crate::attention_feed::AttentionFeed,
    ) -> Result<Arc<Self>> {
        let relaunch = req.clone();
        let Launch {
            mut cmd,
            driver,
            hello,
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
        let session = Self::build(
            req,
            relaunch,
            driver,
            (Some(stdin), None),
            Some(child),
            program,
            None,
            &cache_policies,
            attention,
        );
        let key = known_id.unwrap_or_else(|| format!("{PENDING}{}", uuid::Uuid::new_v4()));
        lock(&registry).insert(key, session.clone());
        session.attention.sessions_changed();
        if let Err(e) = hello.iter().try_for_each(|line| session.send(line)) {
            lock(&registry).retain(|_, s| !Arc::ptr_eq(s, &session));
            session.attention.sessions_changed();
            // No reader thread yet to reap it.
            if let Some(child) = lock(&session.child).as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            return Err(e);
        }
        debug_assert_eq!(session.pid, Some(pid));

        let reader = session.clone();
        std::thread::spawn(move || {
            let mut lines = BufReader::new(stderr);
            loop {
                match bounded_line(&mut lines, 64 * 1024) {
                    Ok(Some(line)) => {
                        tracing::warn!("{} stderr: {line}", reader.program);
                        *lock(&reader.stderr_tail) = strip_ansi(&line);
                    }
                    Ok(None) => break,
                    Err(e) => {
                        reader.fail_io("stderr", e);
                        break;
                    }
                }
            }
        });
        let reader = session.clone();
        std::thread::spawn(move || {
            let mut lines = BufReader::new(stdout);
            loop {
                let line = match bounded_line(&mut lines, 96 * 1024 * 1024) {
                    Ok(Some(line)) => line,
                    Ok(None) => break,
                    Err(e) => {
                        reader.fail_io("output", e);
                        break;
                    }
                };
                reader.apply_line(&line, |new_id, resumed| {
                    rekey(&registry, &reader, new_id, resumed)
                });
            }
            reader.finish();
            lock(&registry).retain(|_, s| !Arc::ptr_eq(s, &reader));
            reader.attention.sessions_changed();
        });
        Ok(session)
    }

    /// A session fed by a terminal's plugin (see [`super::modlink`]); no process is started.
    pub(super) fn attach_mod(
        req: StartAgent,
        driver: Driver,
        link: Arc<ModLink>,
        cache_policies: &Arc<PolicyStore>,
        attention: crate::attention_feed::AttentionFeed,
    ) -> Arc<Self> {
        let ready = req.launch.known_id().map(String::from);
        let relaunch = req.clone();
        Self::build(
            req,
            relaunch,
            driver,
            (None, Some(link)),
            None,
            "claude",
            ready,
            cache_policies,
            attention,
        )
    }

    /// `io` is the process's stdin, or the plugin link that stands in for it.
    #[allow(clippy::too_many_arguments)]
    fn build(
        req: StartAgent,
        relaunch: StartAgent,
        driver: Driver,
        io: (Option<ChildStdin>, Option<Arc<ModLink>>),
        child: Option<Child>,
        program: &'static str,
        ready: Option<String>,
        cache_policies: &Arc<PolicyStore>,
        attention: crate::attention_feed::AttentionFeed,
    ) -> Arc<Self> {
        let known_id = req.launch.known_id().map(String::from);
        let kind = req.launch.kind();
        let cache_policy = match &known_id {
            Some(id) if kind == AgentKind::Claude => cache_policies.get(id),
            _ => CachePolicy::default(),
        };
        let (stdin, link) = io;
        let (outgoing, receiver) = if stdin.is_some() {
            let (tx, rx) = mpsc::sync_channel::<String>(128);
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        let session = Arc::new(Self {
            kind,
            session_id: Mutex::new(known_id.unwrap_or_default()),
            previous_ids: Mutex::new(Vec::new()),
            attention_source: attention.source(),
            attention,
            pane_id: req.pane_id,
            project_path: req.project_path,
            worktree_path: req.worktree_path,
            claude_account_id: req.claude_account_id,
            driver: Mutex::new(driver),
            busy_since: Mutex::new(None),
            updated_at: AtomicU64::new(now_ms()),
            turn_ended_at: Mutex::new(None),
            keepalive_turn: AtomicBool::new(false),
            frames: Frames::new(),
            writer: Mutex::new(()),
            stdin: Mutex::new(stdin),
            outgoing: Mutex::new(outgoing),
            outgoing_bytes: AtomicUsize::new(0),
            pid: child.as_ref().map(Child::id),
            child: Mutex::new(child),
            link,
            exited: AtomicBool::new(false),
            task_files: Mutex::new(HashMap::new()),
            stderr_tail: Mutex::new(String::new()),
            program,
            ready: Mutex::new(ready.map(Ok)),
            ready_cv: Condvar::new(),
            relaunch,
            replaced: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            cache_policy: Mutex::new(cache_policy),
            cache_policies: cache_policies.clone(),
            upkept_for: Mutex::new(None),
            transcript_path: Mutex::new(None),
            early_ttl_reads: Mutex::new((String::new(), EARLY_TTL_READS)),
        });
        let flusher = Arc::downgrade(&session);
        session
            .frames
            .start_flusher(move || flusher.upgrade().map(|s| s.flush()).is_some());
        if let Some(receiver) = receiver {
            let writer = Arc::downgrade(&session);
            std::thread::spawn(move || {
                while let Ok(line) = receiver.recv() {
                    let Some(session) = writer.upgrade() else {
                        break;
                    };
                    let result = session.write_line(&line);
                    session
                        .outgoing_bytes
                        .fetch_sub(line.len(), Ordering::SeqCst);
                    if let Err(e) = result {
                        session.fail_io("write", e);
                        break;
                    }
                }
                if let Some(session) = writer.upgrade() {
                    lock(&session.stdin).take();
                }
            });
            let timer = Arc::downgrade(&session);
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(1));
                let Some(session) = timer.upgrade() else {
                    break;
                };
                if session.has_exited() {
                    break;
                }
                let _ = session.run(|d| Ok(d.tick()));
            });
        }
        session.refresh_attention();
        session
    }

    /// The plugin link of a session fed by a terminal's `claude`.
    pub fn mod_link(&self) -> Option<&Arc<ModLink>> {
        self.link.as_ref()
    }

    /// Whether any client (desktop chat pane, phone) is attached.
    pub fn has_viewers(&self) -> bool {
        self.frames.has_receivers()
    }

    pub fn claude_account_id(&self) -> Option<String> {
        self.claude_account_id.clone()
    }

    pub fn cwd(&self) -> PathBuf {
        PathBuf::from(&self.relaunch.cwd)
    }

    /// Replace a terminal plugin's guessed model list with the CLI's own.
    pub fn pin_models(&self, models: Vec<workbench_core::claude_transcript::ModelOption>) {
        let mut d = lock(&self.driver);
        if let Driver::Claude(t) = &mut *d {
            t.pin_models(models);
            self.broadcast_update(t, &[]);
        }
    }

    /// The Claude session's JSONL under its current id, cached once found.
    fn history_path(&self) -> Option<PathBuf> {
        let super::Launch::Claude { config_dir, .. } = &self.relaunch.launch else {
            return None;
        };
        let id = self.id();
        let mut found = lock(&self.transcript_path);
        match &*found {
            Some((for_id, path)) if *for_id == id => Some(path.clone()),
            _ => {
                let path = super::claude::history(config_dir.as_deref(), &id);
                *found = path.clone().map(|p| (id, p));
                path
            }
        }
    }

    /// The plugin's usage has no `cache_creation` split, so a terminal's
    /// session reads the cache's lifetime from its transcript file each time
    /// a turn ends (its rows are written by then); the CLI may change it.
    /// False when the file names none yet.
    pub(super) fn learn_cache_ttl(&self) -> bool {
        let Some(ttl) = self
            .history_path()
            .and_then(|p| workbench_core::claude_transcript::written_cache_ttl(&p))
        else {
            return false;
        };
        let mut d = lock(&self.driver);
        if let Driver::Claude(t) = &mut *d {
            if t.learn_cache_ttl(ttl) {
                self.broadcast_update(t, &[]);
            }
        }
        true
    }

    /// A terminal's plugin sees a `goal_status` attachment without its
    /// fields: the session's newest such row is read from the transcript file
    /// and folded once row `uuid` is written there, retried briefly until then.
    pub(super) fn learn_goal_status(self: &Arc<Self>, uuid: &str) {
        if self.fold_goal(Some(uuid)) {
            return;
        }
        let (session, uuid) = (Arc::clone(self), uuid.to_string());
        std::thread::spawn(move || {
            for ms in GOAL_RETRY_MS {
                std::thread::sleep(std::time::Duration::from_millis(ms));
                if session.has_exited() || session.fold_goal(Some(&uuid)) {
                    return;
                }
            }
        });
    }

    /// At a turn's end, a goal still shown is checked against the file: a
    /// row the retries above gave up on (a met or cleared goal) isn't left out.
    pub(super) fn recheck_goal(&self) {
        let shown = matches!(&*lock(&self.driver), Driver::Claude(t) if t.meta().goal.is_some());
        if shown {
            self.fold_goal(None);
        }
    }

    fn fold_goal(&self, written: Option<&str>) -> bool {
        let Some(entry) = self
            .history_path()
            .and_then(|p| workbench_core::claude_transcript::goal_status_entry(&p, written))
        else {
            return false;
        };
        self.apply_line(&entry.to_string(), |_, _| {});
        true
    }

    /// An `assistant` line's read, so a first turn doesn't show the 5m
    /// default until its `result`. Bounded per session id: a session that
    /// never writes the cache (caching off) would otherwise scan each line.
    pub(super) fn learn_cache_ttl_early(&self) {
        let id = self.id();
        {
            let mut reads = lock(&self.early_ttl_reads);
            if reads.0 != id {
                *reads = (id, EARLY_TTL_READS);
            }
            if reads.1 == 0 {
                return;
            }
            reads.1 -= 1;
        }
        if self.learn_cache_ttl() {
            lock(&self.early_ttl_reads).1 = 0;
        }
    }

    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// Block until the session has exited, or `timeout` passed.
    pub(super) fn wait_exited(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while !self.has_exited() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
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

    /// The approval or question `request_id` as the session list shows it.
    pub fn waiting_for(&self, request_id: &str) -> Option<WaitingSummary> {
        let d = lock(&self.driver);
        let item = d.view().items().iter().find(|i| i.id() == request_id)?;
        item.waiting_summary()
    }

    /// An approval the terminal's own dialog asks (see [`ModLink::fall_back`]).
    fn terminal_waiting(&self) -> Option<WaitingSummary> {
        self.link.as_ref()?.terminal_waiting()
    }

    pub fn summary(&self) -> AgentSummary {
        let d = lock(&self.driver);
        self.summary_with_view(d.view())
    }

    pub(crate) fn refresh_attention(&self) {
        self.attention
            .observe(self.attention_source, self.summary());
    }

    fn summary_with_view(&self, view: &dyn ChatView) -> AgentSummary {
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
            waiting: view
                .waiting_on()
                .and_then(TranscriptItem::waiting_summary)
                .or_else(|| self.terminal_waiting()),
            running: view
                .running_tool()
                .and_then(TranscriptItem::running_summary),
            previous_ids: lock(&self.previous_ids).clone(),
            terminal_id: self.link.as_ref().and_then(|l| l.terminal_id.clone()),
        }
    }

    /// To the plugin's queue (`/mod/in`), or the process's stdin.
    pub(super) fn send(&self, msg: &Value) -> Result<()> {
        if let Some(link) = &self.link {
            if self.has_exited() {
                bail!("the session has stopped");
            }
            if !link.answer(msg) {
                link.push(msg.clone())?;
            }
            return Ok(());
        }
        let line = msg.to_string();
        let bytes = line.len();
        self.outgoing_bytes
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                n.checked_add(bytes).filter(|v| *v <= 96 * 1024 * 1024)
            })
            .map_err(|_| anyhow::anyhow!("Agent input buffer is full"))?;
        let result = lock(&self.outgoing)
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("the session has stopped"))
            .and_then(|tx| {
                tx.try_send(line)
                    .map_err(|_| anyhow::anyhow!("Agent input buffer is full or closed"))
            });
        if result.is_err() {
            self.outgoing_bytes.fetch_sub(bytes, Ordering::SeqCst);
        }
        result
    }

    fn write_line(&self, msg: &str) -> Result<()> {
        let mut stdin = lock(&self.stdin);
        let Some(pipe) = stdin.as_mut() else {
            bail!("the session has stopped");
        };
        writeln!(pipe, "{msg}").with_context(|| format!("write to {}", self.program))?;
        pipe.flush()
            .with_context(|| format!("flush to {}", self.program))
    }

    fn fail_io(&self, what: &str, error: impl std::fmt::Display) {
        self.frames.fail(
            json!({"t":"error","message":format!("Invalid {} {what}: {error}", self.program)})
                .to_string(),
        );
        if let Some(pid) = self.pid {
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
        }
    }

    /// Publish changes and enqueue effects in protocol order under the driver
    /// lock. Only the separate pipe writer performs blocking process IO.
    fn run(&self, op: impl FnOnce(&mut Driver) -> Result<Effects>) -> Result<()> {
        let _writer = lock(&self.writer);
        let mut d = lock(&self.driver);
        let effects = op(&mut d)?;
        for frame in &effects.frames {
            self.frames.emit(d.view(), frame.to_string());
        }
        if let Some(ready) = effects.ready {
            self.set_ready(ready);
        }
        if effects.snapshot {
            self.touch(d.view());
            self.broadcast_snapshot(d.view());
        } else if effects.meta || !effects.items.is_empty() {
            self.broadcast_update(d.view(), &effects.items);
        }
        for msg in &effects.send {
            if let Err(e) = self.send(msg) {
                // The driver has already advanced. Continuing after dropping an
                // RPC would leave its state out of sync with the owned process.
                if self.pid.is_some() {
                    self.fail_io("write", &e);
                }
                return Err(e);
            }
        }
        Ok(())
    }

    pub fn codex_action(
        &self,
        request_id: &str,
        action: workbench_core::codex_controls::Action,
        params: &Value,
    ) -> Result<()> {
        self.run(|d| d.codex_action(request_id, action, params))
    }

    pub fn prompt(&self, text: &str, images: &[PromptImage], files: &[PromptFile]) -> Result<()> {
        self.keepalive_turn
            .store(text == KEEPALIVE_PROMPT, Ordering::SeqCst);
        let sent = if self.link.is_some() && !(images.is_empty() && files.is_empty()) {
            super::attachment::attachments_saved(&self.id(), text, images, files)
                .and_then(|(text, paths)| self.run(|d| d.prompt_attached(&text, &paths)))
        } else {
            self.run(|d| d.prompt(text, images, files))
        };
        // No turn started, so the flag must not swallow the next one's end.
        if sent.is_err() {
            self.keepalive_turn.store(false, Ordering::SeqCst);
        }
        sent
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
        self.run(|d| d.elicit(request_id, action, content))
    }

    pub fn interrupt(&self) -> Result<()> {
        self.run(Driver::interrupt)
    }

    pub fn set_mode(&self, mode: &str) -> Result<()> {
        self.run(|d| d.set_mode(mode))
    }

    /// The `set_model` request a Claude session was sent, for a restart to repeat.
    pub fn set_model(&self, model: &str) -> Result<Option<Value>> {
        let mut sent = None;
        self.run(|d| {
            let effects = d.set_model(model)?;
            if matches!(d, Driver::Claude(_)) {
                sent = effects.send.first().cloned();
            }
            Ok(effects)
        })?;
        Ok(sent)
    }

    /// Whether a Claude session's model list gives `pick` an id.
    pub fn resolves_model(&self, pick: &str) -> bool {
        match &*lock(&self.driver) {
            Driver::Claude(t) => t.resolve_model(pick).is_some(),
            _ => true,
        }
    }

    /// The effort request a Claude session was sent, for a restart to repeat.
    pub fn set_effort(&self, level: &str) -> Result<Option<Value>> {
        let mut sent = None;
        self.run(|d| {
            let effects = d.set_effort(level)?;
            if matches!(d, Driver::Claude(_)) {
                sent = effects.send.first().cloned();
            }
            Ok(effects)
        })?;
        Ok(sent)
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
        self.frames
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
            bail!("Wait for the current turn to finish, or stop it, first.");
        }
        Ok(meta)
    }

    /// The start request to launch this conversation again, under its current id.
    pub(super) fn relaunch(&self) -> StartAgent {
        let mut req = self.relaunch.clone();
        if let super::Launch::Claude { session_id, .. } = &mut req.launch {
            *session_id = self.id();
        }
        req
    }

    /// Relaunched under the same id: a newer session now runs the conversation.
    pub(super) fn is_replaced(&self) -> bool {
        self.replaced.load(Ordering::SeqCst)
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
            let found = workbench_core::task_output::find(&self.task_file_id(task_id)?)?;
            lock(&self.task_files).insert(task_id.to_string(), found.clone());
            Some(found)
        })?;
        workbench_core::task_output::tail(&path, TASK_OUTPUT_TAIL).ok()
    }

    /// The id a task's files are named by: its `output_id` (a subagent's id)
    /// when it has one, else its own id.
    fn task_file_id(&self, task_id: &str) -> Option<String> {
        let output_id = lock(&self.driver)
            .view()
            .meta()
            .tasks
            .iter()
            .find(|t| t.id == task_id)
            .and_then(|t| t.output_id.clone());
        // A terminal plugin's agent task is keyed by its tool call, which
        // names no file: only its `output_id` can.
        if output_id.is_none() && task_id.starts_with("toolu_") {
            return None;
        }
        Some(output_id.unwrap_or_else(|| task_id.to_string()))
    }

    /// A subagent's own conversation, read from the transcript the CLI keeps
    /// beside the session's: the newest items and how many came before them.
    /// None until that file exists. Claude only.
    pub fn task_transcript(&self, task_id: &str) -> Option<(usize, Vec<TranscriptItem>)> {
        if self.kind != AgentKind::Claude {
            return None;
        }
        let agent_id = self.task_file_id(task_id)?;
        let path = workbench_core::claude_transcript::find_subagent_transcript(
            &self.history_path()?,
            &agent_id,
        )?;
        let transcript = workbench_core::claude_transcript::Transcript::load_subagent(&path);
        let items = transcript.items();
        let start = items.len().saturating_sub(SNAPSHOT_ITEMS);
        Some((start, items[start..].to_vec()))
    }

    /// The whole output of a tool whose chat item carries a preview.
    pub fn artifacts(&self, id: &str) -> Option<Vec<Value>> {
        lock(&self.driver).artifacts(id).map(Vec::from)
    }

    pub fn full_output(&self, tool_id: &str) -> Option<String> {
        lock(&self.driver)
            .view()
            .full_output(tool_id)
            .map(String::from)
    }

    /// The attach snapshot and a receiver for every later frame, taken under
    /// the driver lock so no update falls between them.
    /// Blocking: builds the snapshot under the driver lock.
    pub fn subscribe(&self) -> (String, broadcast::Receiver<Arc<Frame>>) {
        let d = lock(&self.driver);
        let rx = self.frames.subscribe();
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

    /// Apply one line (codex's stdout, or a terminal plugin's post). `alias`
    /// registers the new id when `/clear` or `/resume` (`true`) moves the
    /// conversation — before any client hears of it, so a start or attach
    /// with the new id can never open a second process.
    pub(super) fn apply_line(&self, line: &str, alias: impl FnOnce(&str, bool)) {
        let _writer = lock(&self.writer);
        let mut d = lock(&self.driver);
        let effects = d.apply_line(line);
        for frame in &effects.frames {
            self.frames.emit(d.view(), frame.to_string());
        }
        for msg in &effects.send {
            if let Err(e) = self.send(msg) {
                tracing::warn!("could not answer {}: {e}", self.program);
                if self.pid.is_some() {
                    self.fail_io("write", e);
                }
            }
        }
        if effects.commands {
            let frame = json!({"t": "commands", "commands": d.view().commands()});
            self.frames.emit(d.view(), frame.to_string());
        }
        if let Some(new_id) = effects.new_id {
            let old_id = self.id();
            *lock(&self.session_id) = new_id.clone();
            if effects.resumed {
                // Another conversation: the old ids are free to start again,
                // and the policy is the resumed one's own.
                lock(&self.previous_ids).clear();
                *lock(&self.cache_policy) = self.cache_policies.get(&new_id);
            } else {
                if !old_id.is_empty() && old_id != new_id {
                    lock(&self.previous_ids).push(old_id);
                }
                // The policy follows the conversation to its new id.
                let policy = self.cache_policy();
                if policy != CachePolicy::default() {
                    self.cache_policies.set(&new_id, &policy);
                }
            }
            alias(&new_id, effects.resumed);
            // `/resume` continues a conversation that has history; `/clear`'s has
            // none yet. Read outside the driver lock (`writer` keeps the order).
            if matches!(&*d, Driver::Claude(_)) {
                drop(d);
                let loaded = self
                    .history_path()
                    .map(|path| workbench_core::claude_transcript::Transcript::load(&path));
                d = lock(&self.driver);
                if let (Driver::Claude(t), Some(loaded)) = (&mut *d, loaded) {
                    t.resume_loaded(loaded);
                }
            }
            self.touch(d.view());
            self.broadcast_snapshot(d.view());
            return;
        }
        let view = d.view();
        if let Some(ready) = effects.ready {
            self.set_ready(ready);
        }
        if effects.snapshot {
            self.touch(view);
            self.broadcast_snapshot(view);
        } else if !effects.items.is_empty() || effects.meta {
            self.broadcast_update(view, &effects.items);
        }
    }

    /// Note changes for the next coalesced `update` frame (see `frames`).
    /// Under the driver lock.
    fn broadcast_update(&self, t: &dyn ChatView, changed: &[usize]) {
        self.touch(t);
        self.frames.mark(changed);
    }

    fn flush(&self) {
        let d = lock(&self.driver);
        self.frames.flush(d.view());
    }

    /// Every client gets the whole state again. Under the driver lock.
    fn broadcast_snapshot(&self, t: &dyn ChatView) {
        self.frames.snapshot(self.snapshot(t));
    }

    fn touch(&self, t: &dyn ChatView) {
        let now = now_ms();
        self.updated_at.store(now, Ordering::SeqCst);
        let busy = t.meta().busy;
        let mut since = lock(&self.busy_since);
        // A keep-alive turn isn't work anyone waits on: no "turn complete".
        if since.is_some() && !busy && !self.keepalive_turn.swap(false, Ordering::SeqCst) {
            *lock(&self.turn_ended_at) = Some(now);
        }
        *since = busy.then(|| since.unwrap_or(now));
        drop(since);
        self.attention
            .observe(self.attention_source, self.summary_with_view(t));
    }

    fn finish(&self) {
        lock(&self.outgoing).take();
        let _ = std::fs::remove_dir_all(super::attachment::attachment_dir(&self.id()));
        // Until the leader is reaped below its pid still names its process
        // group: end the background shells it started, which would otherwise
        // outlive it holding ports and files.
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            unsafe {
                libc::killpg(pid as libc::pid_t, libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(pid) = self.pid {
            let _ = workbench_core::shell::command("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .output();
        }
        // End the process before taking the pipe lock: the writer may be
        // blocked because a broken CLI closed stdout and stopped reading stdin.
        lock(&self.stdin).take();
        let code = lock(&self.child)
            .as_mut()
            .and_then(|c| c.wait().ok())
            .and_then(|s| s.code());
        self.exited.store(true, Ordering::SeqCst);
        self.attention.forget(self.attention_source, &self.id());
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
        // The reply's last changes go out before its end.
        self.flush();
        self.frames.end(frame.to_string());
    }

    /// Interrupt, close stdin, and kill the process (and its group) if it
    /// lingers. The reader thread's `finish` reaps it and ends the group.
    pub(super) fn shutdown(&self) {
        // No process of ours: a chat's terminal is the manager's to kill
        // (`AgentManager::stop`), and a detach leaves it running.
        let Some(pid) = self.pid else {
            self.finish();
            return;
        };
        let _ = self.interrupt();
        // Windows has no process groups: `taskkill /T` walks the tree, which it
        // can only do while the process is still alive to be its root.
        #[cfg(windows)]
        {
            std::thread::sleep(Duration::from_millis(500));
            let _ = workbench_core::shell::command("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .output();
        }
        lock(&self.outgoing).take();
        let deadline = Instant::now() + STOP_GRACE;
        while !self.exited.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        if !self.exited.load(Ordering::SeqCst) {
            #[cfg(unix)]
            unsafe {
                libc::killpg(pid as libc::pid_t, libc::SIGKILL);
            }
            #[cfg(windows)]
            if let Some(child) = lock(&self.child).as_mut() {
                let _ = child.kill();
            }
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

/// A malicious/misbehaving CLI cannot allocate an unbounded line. Return an
/// error at the limit and terminate the owned process instead of desyncing RPCs.
fn bounded_line(reader: &mut impl BufRead, limit: usize) -> std::io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let n = reader
        .take((limit + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if n == 0 {
        return Ok(None);
    }
    if n > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "line exceeds size limit",
        ));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
impl AgentSession {
    /// Hold the driver lock as a busy session would.
    pub(crate) fn hold_driver(&self) -> impl Sized + '_ {
        lock(&self.driver)
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod frame_tests;

#[cfg(test)]
mod line_tests {
    use super::bounded_line;
    use std::io::Cursor;
    #[test]
    fn bounded_reader_preserves_framing_and_rejects_oversize_or_invalid_utf8() {
        let mut input = Cursor::new(b"one\ntwo\n");
        assert_eq!(
            bounded_line(&mut input, 4).unwrap().as_deref(),
            Some("one\n")
        );
        assert_eq!(
            bounded_line(&mut input, 4).unwrap().as_deref(),
            Some("two\n")
        );
        assert_eq!(bounded_line(&mut input, 4).unwrap(), None);
        assert!(bounded_line(&mut Cursor::new(b"oversize\n"), 4).is_err());
        assert!(bounded_line(&mut Cursor::new([255, b'\n']), 4).is_err());
    }
}
