//! Chat-mode sessions — an interactive `claude` in a server terminal, run as
//! a chat by the Workbench plugin (`modlink`), or a `codex app-server` over
//! JSON-RPC — whose events fold into the same chat items. Every change is broadcast to attached clients
//! (desktop chat pane, phone) as an `update` frame.
//!
//! One [`AgentManager`] holds both kinds, so ids are global: stopping,
//! attaching and messaging work by id whatever runs behind it. What differs
//! per CLI lives in a driver (`claude`, `codex`); the plumbing in `session`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use workbench_core::claude_transcript::{RunningSummary, WaitingSummary};

mod attachment;
mod cache;
mod claude;
mod codex;
mod driver;
mod modlink;
mod session;

pub use attachment::{PromptFile, PromptImage, MAX_FILES, MAX_IMAGES};
pub use cache::CachePolicy;
pub use modlink::{ModGrant, ModLink};
pub use session::AgentSession;

const DEFAULT_MAX_AGENTS: usize = 16;
/// How long a start waits for codex to open (or resume) its thread.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// How far ahead a chat may be kept warm: every keep-alive turn costs usage.
const MAX_KEEP_WARM_MS: u64 = 24 * 60 * 60 * 1000;

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

#[derive(Clone)]
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

#[derive(Clone)]
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
#[derive(Debug, Clone, Serialize)]
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
    /// The server terminal whose interactive `claude` this chat is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
}

/// Whether a Claude session has a transcript to `--resume` (else `--session-id` starts it).
pub fn claude_history_exists(config_dir: Option<&std::path::Path>, session_id: &str) -> bool {
    claude::history(config_dir, session_id).is_some()
}

type Terminals = OnceLock<(crate::terminal::TerminalManager, u16)>;

fn terminal_alive(terminals: &Terminals, terminal_id: &str) -> bool {
    terminals.get().is_none_or(|(terminals, _)| {
        terminals
            .list()
            .iter()
            .any(|t| t.id == terminal_id && t.alive)
    })
}

/// Drop the grants of terminals that are gone.
fn prune_grants(grants: &Mutex<HashMap<String, ModGrant>>, terminals: &Terminals) {
    lock(grants).retain(|_, g| {
        g.terminal_id
            .as_deref()
            .is_none_or(|t| terminal_alive(terminals, t))
    });
}

#[derive(Clone, Default)]
pub struct AgentManager {
    pub attention: crate::attention_feed::AttentionFeed,
    inner: session::Registry,
    /// Held across a start's check-spawn-insert and a stop's whole shutdown, so
    /// two starts for one id can't both spawn, and a start can't slip in while
    /// a stopped process is still exiting (two writers on one session file).
    lifecycle: Arc<Mutex<()>>,
    cache_policies: Arc<cache::PolicyStore>,
    /// The cache upkeep thread, started with the first session.
    upkeep: Arc<OnceLock<()>>,
    /// Terminal tokens a pane's plugin attaches its `claude` with, by token.
    mod_grants: Arc<Mutex<HashMap<String, ModGrant>>>,
    /// One lock per Claude session id, held across its start (terminal opened,
    /// plugin attached) so two starts of one id can't open two terminals.
    /// Apart from `lifecycle`, which the attach it waits for takes.
    starting: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// The terminals grants are issued for (to drop a dead one's grant) and
    /// the loopback port their plugins reach: set by the first listener.
    terminals: Arc<Terminals>,
}

impl AgentManager {
    pub fn get(&self, session_id: &str) -> Option<Arc<AgentSession>> {
        lock(&self.inner).get(session_id).cloned()
    }

    /// Start a Codex session, or return the one already running for this id;
    /// a new thread is registered once codex has given it an id. (A Claude
    /// chat starts in a terminal: see `agent_routes::claude_start`.)
    pub fn start(&self, req: StartAgent) -> Result<Arc<AgentSession>> {
        let kind = req.launch.kind();
        let Launch::Codex { thread_id, mode } = &req.launch else {
            bail!("Claude chats start in a terminal");
        };
        codex::validate(thread_id.as_deref(), mode.as_deref())?;
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
                    let launch = codex::launch(&req, thread_id.as_deref(), mode.as_deref());
                    self.spawn(req, launch)?
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

    /// Spawn a session (with the cache policy its id had) and make sure upkeep runs.
    fn spawn(&self, req: StartAgent, launch: driver::Launch) -> Result<Arc<AgentSession>> {
        let session = AgentSession::spawn(
            req,
            launch,
            self.inner.clone(),
            self.cache_policies.clone(),
            self.attention.clone(),
        )?;
        self.start_upkeep();
        Ok(session)
    }

    /// The terminals and loopback port terminal plugins use; the first listener wins.
    pub fn bind_terminals(&self, terminals: crate::terminal::TerminalManager, port: u16) {
        self.terminals.get_or_init(|| (terminals, port));
    }

    /// The port a terminal's plugin reaches the server on (the first listener's).
    pub fn mod_port(&self) -> Option<u16> {
        self.terminals.get().map(|(_, port)| *port)
    }

    /// The lock serializing starts of one Claude session id (see `starting`).
    pub fn start_lock(&self, session_id: &str) -> Arc<Mutex<()>> {
        lock(&self.starting)
            .entry(session_id.to_string())
            .or_default()
            .clone()
    }

    /// A token for one terminal's plugin to attach its interactive `claude` with.
    pub fn grant_mod(&self, grant: ModGrant) -> Result<String> {
        let token = workbench_core::token::generate()?;
        lock(&self.mod_grants).insert(token.clone(), grant);
        Ok(token)
    }

    /// Record which terminal a token was issued to.
    pub fn set_grant_terminal(&self, token: &str, terminal_id: &str) {
        if let Some(grant) = lock(&self.mod_grants).get_mut(token) {
            grant.terminal_id = Some(terminal_id.to_string());
        }
    }

    /// Withdraw a terminal's token: its plugin can no longer attach or post.
    pub fn revoke_grant(&self, token: &str) {
        lock(&self.mod_grants).remove(token);
    }

    fn terminal_alive(&self, terminal_id: &str) -> bool {
        terminal_alive(&self.terminals, terminal_id)
    }

    /// Attach (or re-attach) a terminal's `claude` session as a chat, its
    /// history loaded from disk. Another terminal's live session is only taken
    /// over once its link went stale.
    pub fn attach_mod(&self, token: &str, session_id: &str) -> Result<Arc<AgentSession>> {
        claude::validate(session_id, None)?;
        let Some(grant) = lock(&self.mod_grants).get(token).cloned() else {
            bail!("unknown terminal token");
        };
        if grant
            .terminal_id
            .as_deref()
            .is_some_and(|t| !self.terminal_alive(t))
        {
            self.revoke_grant(token);
            bail!("unknown terminal token");
        }
        let _lifecycle = lock(&self.lifecycle);
        if let Some(existing) = self.get(session_id) {
            match existing.mod_link() {
                Some(link) if link.token == token => {
                    link.touch();
                    return Ok(existing);
                }
                // The old terminal's `claude` went without saying so.
                Some(link) if link.is_stale() => {
                    self.forget(&existing);
                    existing.replace();
                }
                Some(_) => bail!("another terminal runs {session_id}"),
                None => bail!("a chat process already runs {session_id}"),
            }
        }
        let config_dir =
            workbench_core::claude_accounts::resolve_saved(grant.claude_account_id.as_deref())?;
        let driver = claude::history_driver(
            config_dir.as_deref(),
            session_id,
            grant.resume_at.as_deref(),
        );
        let req = StartAgent {
            cwd: grant.cwd,
            project_path: grant.project_path,
            worktree_path: grant.worktree_path,
            pane_id: grant.pane_id,
            hook_socket: grant.hook_socket,
            claude_account_id: grant.claude_account_id,
            launch: Launch::Claude {
                session_id: session_id.to_string(),
                permission_mode: grant.permission_mode,
                config_dir,
            },
        };
        let link = Arc::new(ModLink::new(token.to_string(), grant.terminal_id));
        let session = AgentSession::attach_mod(
            req,
            driver,
            link,
            &self.cache_policies,
            self.attention.clone(),
        );
        session.queue(&claude::hello())?;
        lock(&self.inner).insert(session_id.to_string(), session.clone());
        self.start_upkeep();
        Ok(session)
    }

    /// Rewind a terminal session's conversation: its terminal restarts as
    /// `claude --resume <id> --resume-session-at=<fork>` under the same id, and
    /// clients re-attach (`replaced`). Blocking: waits for the new one to attach.
    pub fn rewind_terminal(
        &self,
        terminals: &crate::terminal::TerminalManager,
        session: &Arc<AgentSession>,
        message_id: &str,
    ) -> Result<()> {
        self.restart_terminal(terminals, session, "Rewind", |launch| {
            let Launch::Claude {
                session_id,
                config_dir,
                permission_mode,
            } = launch
            else {
                bail!("Codex chats can't rewind");
            };
            let history = claude::history(config_dir.as_deref(), session_id)
                .ok_or_else(|| anyhow::anyhow!("the session has no history to rewind"))?;
            let fork = workbench_core::claude_transcript::fork_point(&history, message_id)?;
            Ok((Some(fork), permission_mode.clone()))
        })
    }

    /// Switch a terminal session's permission mode: a plugin can't change the
    /// live mode (`$.config.set` writes the settings default), so its terminal
    /// restarts as `claude --resume <id> --permission-mode <mode>`, as a rewind does.
    pub fn mode_terminal(
        &self,
        terminals: &crate::terminal::TerminalManager,
        session: &Arc<AgentSession>,
        mode: &str,
    ) -> Result<()> {
        if !workbench_core::claude_launch::PERMISSION_MODES.contains(&mode) {
            bail!("unknown permission mode: {mode}");
        }
        self.restart_terminal(terminals, session, "Change the mode", |_| {
            Ok((None, Some(mode.to_string())))
        })
    }

    /// Restart an idle terminal session's `claude` under the same id; `plan`
    /// reads its launch and picks where it resumes and the mode it runs in.
    fn restart_terminal(
        &self,
        terminals: &crate::terminal::TerminalManager,
        session: &Arc<AgentSession>,
        what: &str,
        plan: impl FnOnce(&Launch) -> Result<(Option<String>, Option<String>)>,
    ) -> Result<()> {
        let link = session
            .mod_link()
            .context("not a terminal session")?
            .clone();
        // A desktop native terminal's `claude` can't be restarted from here, and
        // a new server terminal beside it would run the session twice.
        if link.terminal_id.is_none() {
            bail!("{what} in this session's own terminal.");
        }
        session.idle_meta()?;
        let req = session.relaunch();
        let (resume_at, permission_mode) = plan(&req.launch)?;
        let Launch::Claude {
            session_id,
            config_dir,
            ..
        } = req.launch
        else {
            bail!("only Claude terminal sessions restart");
        };
        // A session nobody has written to yet has no file to `--resume`.
        let resume = claude_history_exists(config_dir.as_deref(), &session_id);
        // Clients re-attach on `replaced` by starting the session: they wait here
        // for this restart rather than open a second `claude` beside it.
        let starting = self.start_lock(&session_id);
        let _starting = starting.lock().unwrap_or_else(|e| e.into_inner());
        // Hand over before the old `claude` goes: clients re-attach (`replaced`)
        // rather than see it end, and its exit (`bye`) finds nothing to stop.
        {
            let _lifecycle = lock(&self.lifecycle);
            self.forget(session);
            session.replace();
        }
        self.revoke_grant(&link.token);
        // One `claude` per session file: the old one goes before the new one starts.
        if let Some(old) = &link.terminal_id {
            terminals.kill_and_wait(old);
        }
        crate::terminal::create_from_body(
            terminals,
            self,
            crate::terminal::CreateTerminalBody {
                project_path: req.project_path,
                worktree_path: req.worktree_path,
                name: None,
                command: None,
                claude_session: Some(crate::terminal::ClaudeSessionLaunch {
                    id: session_id.clone(),
                    resume,
                    resume_at,
                    permission_mode,
                }),
                cols: 120,
                rows: 40,
                pane_id: req.pane_id,
                hook_socket: req.hook_socket,
                shell: None,
                claude_account_id: req.claude_account_id,
            },
        )?;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            if self.get(&session_id).is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bail!("Claude didn't come back in its terminal after the restart")
    }

    /// The mod session `session_id` attached with `token`, or else the one the
    /// token attached under any id (`/clear` re-keyed it).
    pub fn mod_session(&self, token: &str, session_id: &str) -> Option<Arc<AgentSession>> {
        let owned = |s: &Arc<AgentSession>| s.mod_link().is_some_and(|l| l.token == token);
        self.get(session_id)
            .filter(owned)
            .or_else(|| lock(&self.inner).values().find(|s| owned(s)).cloned())
    }

    /// Fold lines a terminal's plugin posted into its session.
    pub fn feed_mod(&self, session: &Arc<AgentSession>, lines: &[serde_json::Value]) {
        let link = session.mod_link();
        if let Some(link) = link {
            link.touch();
        }
        for line in lines {
            if link.is_some_and(|link| !link.note_line(line)) {
                session.refresh_attention();
                continue;
            }
            session.feed(&line.to_string(), |new_id| {
                lock(&self.inner).insert(new_id.to_string(), session.clone());
            });
            if line.get("type").and_then(serde_json::Value::as_str) == Some("result") {
                session.learn_cache_ttl();
            }
        }
    }

    /// Set a chat's cache policy, saved under its id.
    pub fn set_cache_policy(&self, session: &AgentSession, policy: CachePolicy) -> Result<()> {
        if policy
            .keep_warm_until
            .is_some_and(|until| until > now_ms() + MAX_KEEP_WARM_MS)
        {
            bail!("A chat can be kept warm for at most 24 hours.");
        }
        session.set_cache_policy(policy)
    }

    /// Check every session's cache policy each tick, until the manager is gone.
    fn start_upkeep(&self) {
        self.upkeep.get_or_init(|| {
            let weak = Arc::downgrade(&self.inner);
            let grants = Arc::downgrade(&self.mod_grants);
            let terminals = self.terminals.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(cache::TICK);
                let Some(registry) = weak.upgrade() else {
                    break;
                };
                let mut sessions: Vec<_> = lock(&registry).values().cloned().collect();
                drop(registry);
                sessions.sort_by_key(|s| Arc::as_ptr(s) as usize);
                sessions.dedup_by(|a, b| Arc::ptr_eq(a, b));
                let now = now_ms();
                if let Some(grants) = grants.upgrade() {
                    prune_grants(&grants, &terminals);
                }
                for session in sessions {
                    // The terminal's `claude` quit (or the pane closed) without saying so.
                    if session.mod_link().is_some_and(|l| l.is_stale()) {
                        if let Some(registry) = weak.upgrade() {
                            lock(&registry).retain(|_, s| !Arc::ptr_eq(s, &session));
                        }
                        session.shutdown();
                        continue;
                    }
                    session.upkeep(now);
                }
            });
        });
    }

    /// Stop a session's process (any of its ids). `end`: the person ended the
    /// chat, so other viewers close it, vs a handoff to a terminal. Blocking
    /// (waits out the grace period).
    pub fn stop(&self, session_id: &str, end: bool) -> bool {
        let _lifecycle = lock(&self.lifecycle);
        let Some(session) = self.get(session_id) else {
            return false;
        };
        self.forget(&session);
        if end {
            session.end();
        } else {
            session.shutdown();
        }
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
