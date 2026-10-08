//! Chat-mode sessions — an interactive `claude` in a server terminal, run as
//! a chat by the Workbench plugin (`modlink`), or a `codex app-server` over
//! JSON-RPC — whose events fold into the same chat items. Every change is
//! broadcast to attached clients (desktop chat pane, phone) as an `update` frame.
//!
//! One [`AgentManager`] holds both kinds, so ids are global: stopping,
//! attaching and messaging work by id whatever runs behind it. What differs
//! per CLI lives in a driver (`claude`, `codex`); the plumbing in `session`.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use workbench_core::claude_transcript::{RunningSummary, WaitingSummary};

mod attachment;
mod cache;
mod claude;
mod codex;
mod driver;
mod frames;
mod modlink;
mod session;

pub use attachment::{PromptFile, PromptImage, MAX_FILES, MAX_IMAGES};
pub use cache::CachePolicy;
pub(crate) use claude::validate as validate_claude_session_id;
pub use frames::Frame;
pub use modlink::{ModGrant, ModLink};
pub use session::AgentSession;

const DEFAULT_MAX_AGENTS: usize = 16;
/// How long a start waits for codex to open (or resume) its thread.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a new terminal's `claude` gets to start and attach through the plugin.
const TERMINAL_START: Duration = Duration::from_secs(30);
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
        options: workbench_core::codex_controls::LaunchOptions,
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

/// What came of [`AgentManager::open_terminal`].
pub enum TerminalStart {
    /// The plugin attached the session.
    Attached(Arc<AgentSession>),
    /// The watch stopped the wait with this answer.
    Stopped(serde_json::Value),
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

/// Env vars that hand a terminal's plugin its `/mod` link.
pub type ModEnv = Vec<(&'static str, String)>;

/// A stopped session still exiting, and the ids it had.
type Exiting = (Vec<String>, Arc<AgentSession>);

#[derive(Clone, Default)]
pub struct AgentManager {
    pub attention: crate::attention_feed::AttentionFeed,
    inner: session::Registry,
    /// Held across a start's check-spawn-insert and a stop's unregistering, so
    /// two starts for one id can't both spawn. Never across slow work (a
    /// process's exit, reading history): every start and stop would queue.
    lifecycle: Arc<Mutex<()>>,
    /// Sessions unregistered by a stop whose process may still be exiting, by
    /// the ids they had: a start of one of those ids waits for it to go, so
    /// two processes never write one session file.
    exiting: Arc<Mutex<Vec<Exiting>>>,
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
    /// Ids of sessions the person ended lately, newest last: an attach-only
    /// start on one is told it was ended, not that it's merely gone (a crash).
    ended: Arc<Mutex<VecDeque<String>>>,
    /// The `set_model` a chat last sent each Claude session, by id, and the
    /// ones a restart (rewind, mode) owes its new `claude`, which starts on
    /// the default model: the pick is session-only, so the plugin forgets it.
    model_picks: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    restart_picks: Arc<Mutex<HashMap<String, serde_json::Value>>>,
}

/// How many ended ids are remembered.
const ENDED_KEPT: usize = 64;
/// How long a stopped process may take to be reaped after its grace ran out.
const EXIT_REAP: Duration = Duration::from_secs(2);
/// How long a start waits for a stop of the same id: its grace and reap,
/// plus a Windows `taskkill` before the grace.
const EXIT_WAIT: Duration = Duration::from_secs(10);

impl AgentManager {
    /// Switch a session's model, remembered for a Claude terminal's restart.
    pub fn set_model(&self, session: &AgentSession, model: &str) -> Result<()> {
        if let Some(sent) = session.set_model(model)? {
            lock(&self.model_picks).insert(session.id(), sent);
        }
        Ok(())
    }

    /// A Claude session of `account_id` is linked to its terminal's plugin.
    pub fn has_linked_claude(&self, account_id: &Option<String>) -> bool {
        !self
            .sessions(|s| {
                s.kind == AgentKind::Claude
                    && &s.claude_account_id() == account_id
                    && s.mod_link().is_some_and(|l| !l.is_stale())
            })
            .is_empty()
    }

    /// Give every Claude session of `account_id` in `cwd` a newer model list.
    pub fn pin_models_for(
        &self,
        account_id: &Option<String>,
        cwd: &std::path::Path,
        models: &[workbench_core::claude_transcript::ModelOption],
    ) {
        let found = self.sessions(|s| {
            s.kind == AgentKind::Claude && &s.claude_account_id() == account_id && s.cwd() == cwd
        });
        for session in found {
            session.pin_models(models.to_vec());
        }
    }

    pub fn get(&self, session_id: &str) -> Option<Arc<AgentSession>> {
        lock(&self.inner).get(session_id).cloned()
    }

    /// Start a Codex session, or return the one already running for this id;
    /// a new thread is registered once codex has given it an id. (A Claude
    /// chat starts in a terminal: see `agent_routes::claude_start`.)
    pub fn start(&self, req: StartAgent) -> Result<Arc<AgentSession>> {
        let kind = req.launch.kind();
        let Launch::Codex {
            thread_id,
            mode,
            options,
        } = &req.launch
        else {
            bail!("Claude chats start in a terminal");
        };
        codex::validate(thread_id.as_deref(), mode.as_deref())?;
        options.validate()?;
        let known_id = req.launch.known_id().map(String::from);
        if let Some(id) = &known_id {
            self.wait_exited(id);
        }
        let session = {
            let _lifecycle = lock(&self.lifecycle);
            if let Some(id) = &known_id {
                self.ensure_exited(id)?;
            }
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
                    let launch =
                        codex::launch(&req, thread_id.as_deref(), mode.as_deref(), options.clone());
                    let session = self.spawn(req, launch)?;
                    // A new thread's id is new; a resumed one runs again.
                    if let Some(id) = &known_id {
                        self.unmark_ended(id);
                    }
                    session
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
                    self.attention.sessions_changed();
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
                {
                    let _lifecycle = lock(&self.lifecycle);
                    self.halt(&session, false);
                }
                self.stop_halted(&[session], false);
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

    /// A token for `grant` and the env that hands it to the terminal's plugin
    /// (`WORKBENCH_MOD_URL`/`WORKBENCH_MOD_TOKEN`); None while no loopback
    /// listener serves `/mod`.
    pub fn mod_env(&self, grant: ModGrant) -> Result<Option<(String, ModEnv)>> {
        let Some(port) = self.mod_port() else {
            return Ok(None);
        };
        // The plugin is what makes the terminal a chat; a phone sends no hook
        // socket, which is otherwise what loads it.
        let load_plugin = grant.hook_socket.is_none();
        let token = self.grant_mod(grant)?;
        let mut env = vec![
            ("WORKBENCH_MOD_URL", format!("http://127.0.0.1:{port}")),
            ("WORKBENCH_MOD_TOKEN", token.clone()),
        ];
        if load_plugin {
            if let Some(dirs) = workbench_core::claude_plugin::plugin_dirs_env() {
                env.push((
                    workbench_core::claude_plugin::PLUGIN_DIRS_ENV,
                    dirs.to_string_lossy().into_owned(),
                ));
            }
        }
        Ok(Some((token, env)))
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
        claude::validate(session_id)?;
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
        // Answer what needs no history first: a re-attach, or a refusal.
        self.wait_exited(session_id);
        if let Some(existing) = self.get(session_id) {
            if let Some(link) = existing.mod_link().filter(|l| l.token == token) {
                link.touch();
                return Ok(existing);
            }
            self.takes_over(&existing, token, session_id)?;
        }
        let config_dir =
            workbench_core::claude_accounts::resolve_saved(grant.claude_account_id.as_deref())?;
        // Read before taking the lifecycle lock: a long history would hold up
        // every other start and stop.
        let peeked = grant.resume_at.clone();
        let mut transcript =
            claude::history_transcript(config_dir.as_deref(), session_id, peeked.as_deref());
        let (attached, stale) = {
            let _lifecycle = lock(&self.lifecycle);
            self.ensure_exited(session_id)?;
            let stale = match self.get(session_id) {
                Some(existing) => {
                    if let Some(link) = existing.mod_link().filter(|l| l.token == token) {
                        link.touch();
                        return Ok(existing);
                    }
                    self.takes_over(&existing, token, session_id)?;
                    self.forget(&existing);
                    Some(existing)
                }
                None => None,
            };
            // Only the first attach after a rewind cuts history there: what is
            // typed since continues that branch, which a re-attach must show.
            let resume_at = lock(&self.mod_grants)
                .get_mut(token)
                .and_then(|g| g.resume_at.take());
            if resume_at != peeked {
                // Another attach with this token took the cut meanwhile.
                transcript = claude::history_transcript(
                    config_dir.as_deref(),
                    session_id,
                    resume_at.as_deref(),
                );
            }
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
                driver::Driver::Claude(transcript),
                link,
                &self.cache_policies,
                self.attention.clone(),
            );
            let attached = session.send(&claude::hello()).and_then(|()| {
                if let Some(mut pick) = lock(&self.restart_picks).remove(session_id) {
                    pick["request_id"] = uuid::Uuid::new_v4().to_string().into();
                    session.send(&pick)?;
                }
                lock(&self.inner).insert(session_id.to_string(), session.clone());
                self.unmark_ended(session_id);
                Ok(session)
            });
            (attached, stale)
        };
        // The stale session ends outside the lock: its end waits on its driver lock.
        if let Some(stale) = stale {
            stale.replace();
        }
        let session = attached?;
        self.attention.sessions_changed();
        self.start_upkeep();
        Ok(session)
    }

    /// Whether the attach of `token` may take `session_id` over from `existing`:
    /// only once the old terminal's `claude` went without saying so (stale).
    fn takes_over(&self, existing: &AgentSession, token: &str, session_id: &str) -> Result<()> {
        match existing.mod_link() {
            Some(link) if link.is_stale() && link.token != token => Ok(()),
            Some(_) => bail!("another terminal runs {session_id}"),
            None => bail!("a chat process already runs {session_id}"),
        }
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
        self.restart_terminal(terminals, session, "Rewind", true, |launch| {
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

    /// Restart a terminal session's `claude` under the same id and mode, a
    /// running turn included (a stuck one is what a restart is for): not an End,
    /// so every client re-attaches (`replaced`).
    pub fn restart_session(
        &self,
        terminals: &crate::terminal::TerminalManager,
        session: &Arc<AgentSession>,
    ) -> Result<()> {
        self.restart_terminal(terminals, session, "Restart it", false, |launch| {
            let Launch::Claude {
                permission_mode, ..
            } = launch
            else {
                bail!("only Claude terminal sessions restart");
            };
            Ok((None, permission_mode.clone()))
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
        self.restart_terminal(terminals, session, "Change the mode", true, |_| {
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
        idle_only: bool,
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
        if idle_only {
            session.idle_meta()?;
        }
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
        // A second pick sent while the first restarted waits on its socket, then
        // finds this old session: restarting it again would start a second `claude`.
        if session.is_replaced() {
            bail!("The chat just restarted; try again once it has reconnected.");
        }
        if let Some(pick) = lock(&self.model_picks).get(&session_id).cloned() {
            lock(&self.restart_picks).insert(session_id.clone(), pick);
        }
        // Hand over before the old `claude` goes: clients re-attach (`replaced`)
        // rather than see it end, and its exit (`bye`) finds nothing to stop.
        {
            let _lifecycle = lock(&self.lifecycle);
            self.forget(session);
        }
        // Outside the lock: its end waits on its driver lock. The start lock
        // keeps a re-attach from slipping in meanwhile.
        session.replace();
        self.revoke_grant(&link.token);
        // One `claude` per session file: the old one goes before the new one starts.
        if let Some(old) = &link.terminal_id {
            terminals.kill_and_wait(old);
        }
        let body = crate::terminal::CreateTerminalBody {
            project_path: req.project_path,
            worktree_path: req.worktree_path,
            name: None,
            command: None,
            claude_session: Some(crate::terminal::ClaudeSessionLaunch {
                id: session_id,
                resume,
                resume_at,
                permission_mode,
                prompt: None,
            }),
            cols: 120,
            rows: 40,
            pane_id: req.pane_id,
            hook_socket: req.hook_socket,
            shell: None,
            claude_account_id: req.claude_account_id,
        };
        self.open_terminal(terminals, body, |_| None)?;
        Ok(())
    }

    /// Open a server terminal running `claude` on its `claude_session` and
    /// wait for the plugin to attach it. `watch` runs on each poll with the
    /// terminal's id; an answer from it stops the wait. Blocking.
    pub fn open_terminal(
        &self,
        terminals: &crate::terminal::TerminalManager,
        body: crate::terminal::CreateTerminalBody,
        mut watch: impl FnMut(&str) -> Option<serde_json::Value>,
    ) -> Result<TerminalStart> {
        let session_id = body
            .claude_session
            .as_ref()
            .map(|c| c.id.clone())
            .context("not a Claude session")?;
        let terminal = crate::terminal::create_from_body(terminals, self, body)?;
        let deadline = Instant::now() + TERMINAL_START;
        while Instant::now() < deadline {
            if let Some(session) = self.get(&session_id) {
                return Ok(TerminalStart::Attached(session));
            }
            if let Some(answer) = watch(&terminal.id) {
                return Ok(TerminalStart::Stopped(answer));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // Not attached: one left running would be a second `claude` on the session.
        terminals.kill(&terminal.id);
        bail!(
            "Claude didn't start in its terminal within {}s. Open it as a terminal to see why (a login, an error).",
            TERMINAL_START.as_secs()
        )
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
            if let Some(link) = link {
                link.note_line(line);
            }
            self.forget_replaced_pick(session, line);
            session.apply_line(&line.to_string(), |new_id, resumed| {
                session::rekey(&self.inner, session, new_id, resumed)
            });
            match line.get("type").and_then(serde_json::Value::as_str) {
                Some("result") => {
                    session.learn_cache_ttl();
                }
                Some("assistant") => session.learn_cache_ttl_early(),
                _ => {}
            }
        }
    }

    /// A model picked in the TUI (a `system:init` naming another pick) replaces
    /// the chat's, so a restart doesn't bring the chat's back.
    fn forget_replaced_pick(&self, session: &AgentSession, line: &serde_json::Value) {
        let Some(choice) = line
            .get("modelChoice")
            .and_then(serde_json::Value::as_str)
            .filter(|_| line["type"] == "system" && line["subtype"] == "init")
        else {
            return;
        };
        let mut picks = lock(&self.model_picks);
        let id = session.id();
        if picks
            .get(&id)
            .is_some_and(|p| p["request"]["model"] != choice)
        {
            picks.remove(&id);
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
            let attention = self.attention.clone();
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
                        attention.sessions_changed();
                        session.shutdown();
                        continue;
                    }
                    session.upkeep(now);
                }
            });
        });
    }

    /// Stop a session's process (any of its ids); a Claude chat's process is
    /// its terminal's `claude`, so that terminal goes too. `end`: the person
    /// ended the chat, so other viewers close it, vs a handoff to a terminal.
    /// Blocking (waits out the grace period).
    pub fn stop(&self, session_id: &str, end: bool) -> bool {
        let session = {
            let _lifecycle = lock(&self.lifecycle);
            let Some(session) = self.get(session_id) else {
                return false;
            };
            self.halt(&session, end);
            session
        };
        self.stop_halted(std::slice::from_ref(&session), end);
        self.kill_terminal(&session);
        true
    }

    /// The terminal's `claude` left (it exited, or `/exit`): drop its chat and
    /// keep the terminal, which is the person's shell again.
    pub fn detach(&self, session: &Arc<AgentSession>) {
        {
            let _lifecycle = lock(&self.lifecycle);
            if !self
                .sessions(|_| true)
                .iter()
                .any(|s| Arc::ptr_eq(s, session))
            {
                return;
            }
            self.halt(session, false);
        }
        self.stop_halted(std::slice::from_ref(session), false);
    }

    /// Stop whatever chat sessions (either kind) a closed pane owned; `end` as
    /// for [`Self::stop`]. Blocking.
    pub fn stop_pane(&self, pane_id: &str, end: bool) -> usize {
        let owned = self.halt_matching(|s| s.pane_id.as_deref() == Some(pane_id), end);
        self.stop_halted(&owned, end);
        for session in &owned {
            self.kill_terminal(session);
        }
        owned.len()
    }

    /// A terminal is being closed on purpose: End the chats it hosts first, so
    /// every device sees an End, not an exit. Blocking.
    pub fn end_terminal(&self, terminal_id: &str) {
        let hosted = self.halt_matching(
            |s| {
                s.mod_link()
                    .is_some_and(|l| l.terminal_id.as_deref() == Some(terminal_id))
            },
            true,
        );
        self.stop_halted(&hosted, true);
    }

    /// [`Self::halt`] every session matching `keep`, under the lifecycle lock.
    fn halt_matching(
        &self,
        keep: impl Fn(&AgentSession) -> bool,
        end: bool,
    ) -> Vec<Arc<AgentSession>> {
        let _lifecycle = lock(&self.lifecycle);
        let found = self.sessions(keep);
        for session in &found {
            self.halt(session, end);
        }
        found
    }

    fn kill_terminal(&self, session: &AgentSession) {
        let terminal = session.mod_link().and_then(|l| l.terminal_id.as_deref());
        if let (Some(id), Some((terminals, _))) = (terminal, self.terminals.get()) {
            terminals.kill(id);
        }
    }

    /// Stop every session (the app is quitting or installing an update). Blocking.
    pub fn kill_all(&self) {
        let all = self.halt_matching(|_| true, false);
        lock(&self.inner).clear();
        self.attention.sessions_changed();
        self.stop_halted(&all, false);
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

    /// Unregister a session, held as exiting under its ids until
    /// [`Self::stop_halted`] has stopped it; an `end` is remembered by its ids.
    /// Under the lifecycle lock.
    fn halt(&self, session: &Arc<AgentSession>, end: bool) {
        let mut ids: Vec<String> = lock(&self.inner)
            .iter()
            .filter(|(_, s)| Arc::ptr_eq(s, session))
            .map(|(id, _)| id.clone())
            .collect();
        let id = session.id();
        if !id.is_empty() && !ids.contains(&id) {
            ids.push(id);
        }
        if end {
            let mut ended = lock(&self.ended);
            ended.extend(ids.iter().filter(|id| !session::is_pending(id)).cloned());
            let excess = ended.len().saturating_sub(ENDED_KEPT);
            ended.drain(..excess);
        }
        self.forget(session);
        lock(&self.exiting).push((ids, session.clone()));
    }

    /// Stop sessions [`Self::halt`] unregistered, all at once and outside the
    /// lifecycle lock: each process gets a grace period to exit.
    fn stop_halted(&self, sessions: &[Arc<AgentSession>], end: bool) {
        let stop = |session: &Arc<AgentSession>| {
            if end {
                session.end();
            } else {
                session.shutdown();
            }
            // A process killed at the end of its grace is reaped just after.
            session.wait_exited(EXIT_REAP);
            lock(&self.exiting).retain(|(_, s)| !Arc::ptr_eq(s, session));
        };
        match sessions {
            [] => {}
            [one] => stop(one),
            many => std::thread::scope(|scope| {
                for session in many {
                    scope.spawn(move || stop(session));
                }
            }),
        }
    }

    fn exiting(&self, session_id: &str) -> bool {
        lock(&self.exiting)
            .iter()
            .any(|(ids, _)| ids.iter().any(|id| id == session_id))
    }

    /// Wait (not under the lifecycle lock) while a stop of `session_id` is under way.
    fn wait_exited(&self, session_id: &str) {
        let deadline = Instant::now() + EXIT_WAIT;
        while self.exiting(session_id) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Under the lifecycle lock, after [`Self::wait_exited`].
    fn ensure_exited(&self, session_id: &str) -> Result<()> {
        if self.exiting(session_id) {
            bail!("{session_id} is still stopping; try again in a moment");
        }
        Ok(())
    }

    /// The session under this id was ended by the person and not started since.
    pub fn was_ended(&self, session_id: &str) -> bool {
        lock(&self.ended).iter().any(|id| id == session_id)
    }

    fn unmark_ended(&self, session_id: &str) {
        lock(&self.ended).retain(|id| id != session_id);
    }

    /// Drop every id (aliases included) that points at this session.
    fn forget(&self, session: &Arc<AgentSession>) {
        lock(&self.inner).retain(|_, s| !Arc::ptr_eq(s, session));
        self.attention.sessions_changed();
    }

    fn live_count(&self) -> usize {
        // A stopped process still exiting is still running.
        self.sessions(|_| true).len() + lock(&self.exiting).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_env_hands_a_terminal_its_own_token_on_loopback() {
        let grant = || ModGrant {
            pane_id: Some("pane".into()),
            project_path: "/p".into(),
            worktree_path: None,
            claude_account_id: None,
            cwd: "/p".into(),
            hook_socket: Some("/hook.sock".into()),
            resume_at: None,
            permission_mode: None,
            terminal_id: None,
        };
        let agents = AgentManager::default();
        assert!(
            agents.mod_env(grant()).unwrap().is_none(),
            "no listener yet"
        );
        agents.bind_terminals(crate::terminal::TerminalManager::default(), 4321);
        let (token, env) = agents.mod_env(grant()).unwrap().unwrap();
        assert_eq!(
            env,
            [
                ("WORKBENCH_MOD_URL", "http://127.0.0.1:4321".to_string()),
                ("WORKBENCH_MOD_TOKEN", token.clone()),
            ]
        );
        assert!(lock(&agents.mod_grants).contains_key(&token));
        let (other, _) = agents.mod_env(grant()).unwrap().unwrap();
        assert_ne!(token, other, "each terminal gets its own");
    }
}
