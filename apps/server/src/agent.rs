//! Chat-mode sessions: one CLI process per session — `claude -p` over
//! stream-json, or `codex app-server` over JSON-RPC — whose events fold into
//! the same chat items. Every change is broadcast to attached clients
//! (desktop chat pane, phone) as an `update` frame.
//!
//! One [`AgentManager`] holds both kinds, so ids are global: stopping,
//! attaching and messaging work by id whatever runs behind it. What differs
//! per CLI lives in a driver (`claude`, `codex`); the plumbing in `session`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use workbench_core::claude_transcript::{RunningSummary, WaitingSummary};

mod attachment;
mod claude;
mod codex;
mod driver;
mod session;

pub use attachment::{PromptFile, PromptImage, MAX_FILES, MAX_IMAGES};
pub use session::AgentSession;

const DEFAULT_MAX_AGENTS: usize = 16;
/// How long a start waits for codex to open (or resume) its thread.
const READY_TIMEOUT: Duration = Duration::from_secs(30);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Which CLI a chat session runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
}

pub struct StartAgent {
    pub cwd: String,
    /// As the client gave them, for listing; `cwd` is what they resolved to.
    pub project_path: String,
    pub worktree_path: Option<String>,
    /// Forwarded as `WORKBENCH_PANE_ID` / `WORKBENCH_HOOK_SOCKET` so hooks keep
    /// driving the desktop's activity tracking, as for terminal panes.
    pub pane_id: Option<String>,
    pub hook_socket: Option<String>,
    /// The Claude account id `config_dir` was resolved from, for listing.
    pub claude_account_id: Option<String>,
    pub launch: Launch,
}

pub enum Launch {
    Claude {
        session_id: String,
        permission_mode: Option<String>,
        /// The Claude account's config dir (`CLAUDE_CONFIG_DIR`); `None` is the default login.
        config_dir: Option<PathBuf>,
    },
    Codex {
        /// The thread to resume; `None` starts a new one.
        thread_id: Option<String>,
        /// A `CodexMode`; `None` leaves `~/.codex/config.toml` in charge.
        mode: Option<String>,
    },
}

impl Launch {
    pub fn kind(&self) -> AgentKind {
        match self {
            Self::Claude { .. } => AgentKind::Claude,
            Self::Codex { .. } => AgentKind::Codex,
        }
    }

    /// The id known before spawning (Claude's, or the Codex thread to resume).
    fn known_id(&self) -> Option<&str> {
        match self {
            Self::Claude { session_id, .. } => Some(session_id),
            Self::Codex { thread_id, .. } => thread_id.as_deref(),
        }
    }
}

/// One live session as the phone's home screen lists it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSummary {
    pub agent: AgentKind,
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
    /// Unix ms when the last turn went idle; null before the first one ends.
    pub turn_ended_at: Option<u64>,
    pub waiting: Option<WaitingSummary>,
    pub running: Option<RunningSummary>,
    /// Ids it ran under before a `/clear`, so a client holding one follows the re-key.
    pub previous_ids: Vec<String>,
}

#[derive(Clone, Default)]
pub struct AgentManager {
    inner: session::Registry,
    /// Held across a start's check-spawn-insert and a stop's whole shutdown, so
    /// two starts for one id can't both spawn, and a start can't slip in while
    /// a stopped process is still exiting (two writers on one session file).
    lifecycle: Arc<Mutex<()>>,
}

impl AgentManager {
    pub fn get(&self, session_id: &str) -> Option<Arc<AgentSession>> {
        lock(&self.inner).get(session_id).cloned()
    }

    /// Start a session, or return the one already running for this id. A
    /// new Codex thread is registered once codex has given it an id.
    pub fn start(&self, req: StartAgent) -> Result<Arc<AgentSession>> {
        let kind = req.launch.kind();
        match &req.launch {
            Launch::Claude {
                session_id,
                permission_mode,
                ..
            } => claude::validate(session_id, permission_mode.as_deref())?,
            Launch::Codex { thread_id, mode } => {
                codex::validate(thread_id.as_deref(), mode.as_deref())?
            }
        }
        let known_id = req.launch.known_id().map(String::from);
        let session = {
            let _lifecycle = lock(&self.lifecycle);
            match known_id.as_deref().and_then(|id| self.get(id)) {
                Some(existing) if existing.kind != kind => {
                    bail!("{} is another kind of chat", existing.id())
                }
                Some(existing) => existing,
                None => {
                    let max = std::env::var("WORKBENCH_MAX_AGENTS")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(DEFAULT_MAX_AGENTS);
                    if self.live_count() >= max {
                        bail!("chat session limit reached ({max})");
                    }
                    let launch = match &req.launch {
                        Launch::Claude {
                            session_id,
                            permission_mode,
                            config_dir,
                        } => claude::launch(
                            &req,
                            session_id,
                            permission_mode.as_deref(),
                            config_dir.as_deref(),
                        ),
                        Launch::Codex { thread_id, mode } => {
                            codex::launch(&req, thread_id.as_deref(), mode.as_deref())
                        }
                    };
                    AgentSession::spawn(req, launch, self.inner.clone())?
                }
            }
        };
        // Outside the lifecycle lock: a slow codex must not hold up every
        // other start and stop.
        match session.wait_ready(READY_TIMEOUT) {
            Ok(id) => {
                if known_id.is_none() {
                    {
                        let mut inner = lock(&self.inner);
                        inner.insert(id, session.clone());
                        inner.retain(|k, s| !(session::is_pending(k) && Arc::ptr_eq(s, &session)));
                    }
                    // The reader drops an exited session from the map; one
                    // that exited before this insert is dropped here instead.
                    if session.has_exited() {
                        self.forget(&session);
                        bail!("codex exited as the thread started");
                    }
                }
                Ok(session)
            }
            Err(e) => {
                let _lifecycle = lock(&self.lifecycle);
                self.forget(&session);
                session.shutdown();
                Err(e)
            }
        }
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

    /// Stop whatever chat sessions (either kind) a closed pane owned. Blocking.
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

    /// Every live session of `kind` (or of both), most recently changed first.
    pub fn summaries(&self, kind: Option<AgentKind>) -> Vec<AgentSummary> {
        let mut grouped: Vec<(Arc<AgentSession>, Vec<String>)> = Vec::new();
        for (id, session) in lock(&self.inner).iter() {
            // A new Codex thread is listed once it has an id to attach to.
            if kind.is_some_and(|k| k != session.kind) || session::is_pending(id) {
                continue;
            }
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
