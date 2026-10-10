//! Chat-mode sessions — an interactive `claude` in a server terminal, run as
//! a chat by the Workbench plugin (`modlink`), or a `codex app-server` over
//! JSON-RPC — whose events fold into the same chat items. Every change is
//! broadcast to attached clients (desktop chat pane, phone) as an `update` frame.
//!
//! One [`AgentManager`] holds both kinds, so ids are global: stopping,
//! attaching and messaging work by id whatever runs behind it. What differs
//! per CLI lives in a driver (`claude`, `codex`); the plumbing in `session`.

use std::collections::HashMap;
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

pub(crate) fn now_ms() -> u64 {
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
    /// Forwarded as `WORKBENCH_PANE_ID`, as for terminal panes.
    pub pane_id: Option<String>,
    /// The server's own hook bridge ([`AgentManager::hooks`]); whatever a
    /// caller put here is replaced before anything spawns.
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
#[derive(Debug, Clone, PartialEq, Serialize)]
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

/// Between Claude Code's trust dialog appearing and it reading keys.
const TRUST_SETTLE: Duration = Duration::from_secs(1);
/// After answering the trust dialog, how long before answering once more.
const TRUST_RETRY: Duration = Duration::from_secs(5);

/// An [`AgentManager::open_terminal`] watch that answers Claude Code's folder
/// trust dialog, which comes before any plugin loads (again once if `claude`
/// still hasn't attached).
fn trust_watch(terminals: &crate::terminal::TerminalManager) -> impl FnMut(&str) + '_ {
    let mut answers: Vec<Instant> = Vec::new();
    move |terminal| {
        let answer_again = answers.len() == 1 && answers[0].elapsed() > TRUST_RETRY;
        let asks = (answers.is_empty() || answer_again)
            && terminals
                .recent_output(terminal)
                .is_some_and(|out| workbench_core::claude_launch::shows_trust_prompt(&out));
        if !asks {
            return;
        }
        // Keys typed as the dialog first draws are lost.
        std::thread::sleep(TRUST_SETTLE);
        for keys in workbench_core::claude_launch::TRUST_ACCEPT_KEYS {
            terminals.type_keys(terminal, keys);
            std::thread::sleep(Duration::from_millis(300));
        }
        answers.push(Instant::now());
    }
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
    /// Where every terminal and chat this manager starts reports its hooks.
    pub hooks: crate::hook_bridge::HookBridge,
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
    /// One lock per session id, held by every spawn and stop of it: the
    /// workspace service's (`workspace/exec.rs`) and a rewind, mode or account
    /// restart's (terminal opened, plugin attached), so none of them runs one
    /// session in two processes. Apart from `lifecycle`, which the attach takes.
    starting: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// The terminals grants are issued for (to drop a dead one's grant) and
    /// the loopback port their plugins reach: set by the first listener.
    terminals: Arc<Terminals>,
    /// The `set_model` and effort a chat last sent each Claude session, by id,
    /// and the ones a restart (rewind, mode) owes its new `claude`, which starts
    /// on the defaults: the picks are session-only, so the plugin forgets them
    /// (its worker restarting too, which attaches again with the same token).
    model_picks: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    effort_picks: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    restart_picks: Arc<Mutex<HashMap<String, Vec<serde_json::Value>>>>,
}

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

    /// Switch a session's effort, remembered as a model pick is.
    pub fn set_effort(&self, session: &AgentSession, level: &str) -> Result<()> {
        if let Some(sent) = session.set_effort(level)? {
            lock(&self.effort_picks).insert(session.id(), sent);
        }
        Ok(())
    }

    /// The picks a session's `claude` is owed, model first, each with a fresh request id.
    fn picks(&self, session_id: &str) -> Vec<serde_json::Value> {
        [&self.model_picks, &self.effort_picks]
            .into_iter()
            .filter_map(|picks| lock(picks).get(session_id).cloned())
            .collect()
    }

    fn send_picks(&self, session: &AgentSession, picks: Vec<serde_json::Value>) -> Result<()> {
        for mut pick in picks {
            pick["request_id"] = uuid::Uuid::new_v4().to_string().into();
            session.send(&pick)?;
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
    pub fn start(&self, mut req: StartAgent) -> Result<Arc<AgentSession>> {
        req.hook_socket = self.hooks.socket();
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
                    self.halt(&session);
                }
                self.stop_halted(&[session]);
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
        let mut first = false;
        self.terminals.get_or_init(|| {
            first = true;
            (terminals, port)
        });
        if first {
            self.hooks.start();
            self.mirror_codex_notify();
        }
    }

    /// A terminal Codex's `notify` goes to the attention feed. A chat's
    /// completions are already published by its app-server driver.
    fn mirror_codex_notify(&self) {
        let (agents, mut events) = (self.clone(), self.hooks.subscribe());
        std::thread::spawn(move || loop {
            use tokio::sync::broadcast::error::RecvError;
            let event = match events.blocking_recv() {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            let crate::hook_bridge::HookEvent::Codex { pane_id, codex } = event else {
                continue;
            };
            if let Some((thread, cwd)) = crate::hook_bridge::codex_turn_ended(&codex) {
                agents.codex_notified(&pane_id, thread, cwd);
            }
        });
    }

    fn codex_notified(&self, pane_id: &str, session_id: &str, cwd: &str) {
        if self.get(session_id).is_some() {
            return;
        }
        let terminal = self
            .terminals
            .get()
            .and_then(|(terminals, _)| terminals.terminal_for_pane(pane_id));
        self.attention.publish(crate::attention::Attention {
            kind: crate::attention::AttentionKind::TurnEnded,
            agent: AgentKind::Codex,
            session_id: session_id.into(),
            previous_ids: Vec::new(),
            pane_id: Some(pane_id.into()),
            title: terminal.as_ref().and_then(|t| t.name.clone()),
            terminal_id: terminal.map(|t| t.id),
            project_path: cwd.into(),
            worktree_path: None,
            claude_account_id: None,
            waiting: None,
            busy: false,
            terminal_only: true,
        });
    }

    /// The port a terminal's plugin reaches the server on (the first listener's).
    pub fn mod_port(&self) -> Option<u16> {
        self.terminals.get().map(|(_, port)| *port)
    }

    /// The lock serializing spawns and stops of one session id (see `starting`).
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
    /// (`WORKBENCH_MOD_URL`/`WORKBENCH_MOD_TOKEN`, the plugin dirs, the hook
    /// bridge); None while no loopback listener serves `/mod`.
    pub fn mod_env(&self, grant: ModGrant) -> Result<Option<(String, ModEnv)>> {
        let Some(port) = self.mod_port() else {
            return Ok(None);
        };
        let token = self.grant_mod(grant)?;
        let mut env = vec![
            ("WORKBENCH_MOD_URL", format!("http://127.0.0.1:{port}")),
            ("WORKBENCH_MOD_TOKEN", token.clone()),
        ];
        // The plugin is what makes the terminal a chat, and reports its hooks.
        if let Some(dirs) = workbench_core::claude_plugin::plugin_dirs_env() {
            env.push((
                workbench_core::claude_plugin::PLUGIN_DIRS_ENV,
                dirs.to_string_lossy().into_owned(),
            ));
        }
        if let Some(socket) = self.hooks.socket() {
            env.push(("WORKBENCH_HOOK_SOCKET", socket));
        }
        Ok(Some((token, env)))
    }

    /// A terminal opened for a session may also attach as the id its
    /// `/clear` or `/resume` moved it to; a plain shell is never pinned.
    fn bind_grant(&self, token: &str, session_id: &str) {
        if let Some(grant) = lock(&self.mod_grants).get_mut(token) {
            if !grant.session_ids.is_empty() && !grant.session_ids.iter().any(|id| id == session_id)
            {
                grant.session_ids.push(session_id.to_string());
            }
        }
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
        // Anything in the terminal holds its token (a prompt-injected Bash
        // too): a terminal opened for a session attaches as nothing else.
        if !grant.session_ids.is_empty() && !grant.session_ids.iter().any(|id| id == session_id) {
            bail!("this terminal was opened for another session");
        }
        // Answer what needs no history first: a re-attach, or a refusal.
        self.wait_exited(session_id);
        if let Some(existing) = self.get(session_id) {
            // The plugin's worker restarted (a hot reload, a respawn): its
            // session-only picks went with it.
            if let Some(link) = existing.mod_link().filter(|l| l.has_token(token)) {
                link.touch();
                self.send_picks(&existing, self.picks(session_id))?;
                return Ok(existing);
            }
            self.takes_over(&existing, token, session_id)?;
        }
        let config_dir =
            workbench_core::claude_accounts::resolve_saved(grant.claude_account_id.as_deref())?;
        // Read before taking the lifecycle lock: a long history would hold up
        // every other start and stop.
        let mut peeked = grant.resume_at.clone();
        let (attached, stale) = loop {
            let transcript =
                claude::history_transcript(config_dir.as_deref(), session_id, peeked.as_deref());
            let _lifecycle = lock(&self.lifecycle);
            // Another attach with this token took the rewind cut meanwhile:
            // read again, outside the lock.
            let now = lock(&self.mod_grants)
                .get(token)
                .and_then(|g| g.resume_at.clone());
            if now != peeked {
                peeked = now;
                continue;
            }
            self.ensure_exited(session_id)?;
            let stale = match self.get(session_id) {
                Some(existing) => {
                    if let Some(link) = existing.mod_link().filter(|l| l.has_token(token)) {
                        link.touch();
                        self.send_picks(&existing, self.picks(session_id))?;
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
            // Taken once nothing above can refuse the attach, so a retried
            // hello still gets the cut.
            let cut = lock(&self.mod_grants)
                .get_mut(token)
                .and_then(|g| g.resume_at.take());
            let transcript = if cut == peeked {
                transcript
            } else {
                claude::history_transcript(config_dir.as_deref(), session_id, cut.as_deref())
            };
            let req = StartAgent {
                cwd: grant.cwd,
                project_path: grant.project_path,
                worktree_path: grant.worktree_path,
                pane_id: grant.pane_id,
                hook_socket: self.hooks.socket(),
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
                let owed = lock(&self.restart_picks).remove(session_id);
                self.send_picks(&session, owed.unwrap_or_default())?;
                lock(&self.inner).insert(session_id.to_string(), session.clone());
                Ok(session)
            });
            break (attached, stale);
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
            Some(link) if link.is_stale() && !link.has_token(token) => Ok(()),
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
        self.restart_terminal(terminals, session, "Rewind", true, None, |launch| {
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
        self.restart_terminal(terminals, session, "Restart it", false, None, |launch| {
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
        self.restart_terminal(terminals, session, "Change the mode", true, None, |_| {
            Ok((None, Some(mode.to_string())))
        })
    }

    /// Move an idle terminal session to another Claude account: its terminal
    /// restarts under that login's `CLAUDE_CONFIG_DIR`, resuming the session
    /// moved there, so a chat at one account's limit carries on under another.
    pub fn account_terminal(
        &self,
        terminals: &crate::terminal::TerminalManager,
        session: &Arc<AgentSession>,
        account_id: Option<String>,
    ) -> Result<()> {
        if session.claude_account_id() == account_id {
            return Ok(());
        }
        let config_dir = workbench_core::claude_accounts::resolve_saved(account_id.as_deref())?;
        // Refused before the restart, so the chat keeps running where it is.
        if claude_history_exists(config_dir.as_deref(), &session.id()) {
            bail!("That account already has this session; resume it from there instead.");
        }
        let to = Some((account_id, config_dir));
        self.restart_terminal(
            terminals,
            session,
            "Switch the account",
            true,
            to,
            |launch| {
                let Launch::Claude {
                    permission_mode, ..
                } = launch
                else {
                    bail!("only Claude chats switch accounts");
                };
                Ok((None, permission_mode.clone()))
            },
        )
    }

    /// Restart an idle terminal session's `claude` under the same id, or under
    /// the account `to` names; `plan` reads its launch and picks where it
    /// resumes and the mode it runs in.
    fn restart_terminal(
        &self,
        terminals: &crate::terminal::TerminalManager,
        session: &Arc<AgentSession>,
        what: &str,
        idle_only: bool,
        to: Option<(Option<String>, Option<std::path::PathBuf>)>,
        plan: impl FnOnce(&Launch) -> Result<(Option<String>, Option<String>)>,
    ) -> Result<()> {
        let link = session
            .mod_link()
            .context("not a terminal session")?
            .clone();
        // A new server terminal beside a native one would run the session twice.
        if self.own_terminal(session).is_none() {
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
        // Serialized with the workspace's spawns and stops of this session (`start_lock`).
        let starting = self.start_lock(&session_id);
        let _starting = starting.lock().unwrap_or_else(|e| e.into_inner());
        // A second pick sent while the first restarted waits on its socket, then
        // finds this old session: restarting it again would start a second `claude`.
        if session.is_replaced() {
            bail!("The chat just restarted; try again once it has reconnected.");
        }
        if to.is_some() {
            // Picks resolved against the old login's models may not exist on the new one.
            lock(&self.model_picks).remove(&session_id);
            lock(&self.effort_picks).remove(&session_id);
        }
        let owed = self.picks(&session_id);
        if !owed.is_empty() {
            lock(&self.restart_picks).insert(session_id.clone(), owed);
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
        let open = |account_id: Option<String>, config_dir: Option<&std::path::Path>| {
            // A session nobody has written to yet has no file to `--resume`.
            let resume = claude_history_exists(config_dir, &session_id);
            crate::terminal::CreateTerminalBody {
                project_path: req.project_path.clone(),
                worktree_path: req.worktree_path.clone(),
                name: None,
                command: None,
                claude_session: Some(crate::terminal::ClaudeSessionLaunch {
                    id: session_id.clone(),
                    resume,
                    resume_at: resume_at.clone(),
                    permission_mode: permission_mode.clone(),
                    prompt: None,
                }),
                cols: 120,
                rows: 40,
                pane_id: req.pane_id.clone(),
                shell: None,
                claude_account_id: account_id,
                codex_session: None,
                native: false,
            }
        };
        let Some((to_id, to_dir)) = to else {
            self.open_terminal(
                terminals,
                open(req.claude_account_id.clone(), config_dir.as_deref()),
                |_| {},
            )?;
            return Ok(());
        };
        // The new login resumes the session once its files are in its config
        // dir. This folder ran under the old login, so its trust dialog (kept
        // per config dir) is answered rather than asked again.
        let switched = workbench_core::claude_accounts::move_session(
            config_dir.as_deref(),
            to_dir.as_deref(),
            &session_id,
        )
        .context("the session's files couldn't move to that account")
        .and_then(|()| {
            let body = open(to_id, to_dir.as_deref());
            self.open_terminal(terminals, body, trust_watch(terminals))
        });
        let Err(e) = switched else {
            return Ok(());
        };
        // Back where it was, or a start under the old login would find no
        // history and open an empty conversation under the same id.
        if let Err(back) = workbench_core::claude_accounts::move_session(
            to_dir.as_deref(),
            config_dir.as_deref(),
            &session_id,
        ) {
            tracing::warn!("moving {session_id} back after a failed account switch: {back}");
        }
        self.open_terminal(
            terminals,
            open(req.claude_account_id.clone(), config_dir.as_deref()),
            |_| {},
        )?;
        Err(e.context("Couldn't switch the account; the chat carries on where it was"))
    }

    /// Open a server terminal running `claude` on its `claude_session` and
    /// wait for the plugin to attach it. `watch` runs on each poll with the
    /// terminal's id. Blocking.
    fn open_terminal(
        &self,
        terminals: &crate::terminal::TerminalManager,
        body: crate::terminal::CreateTerminalBody,
        mut watch: impl FnMut(&str),
    ) -> Result<Arc<AgentSession>> {
        let session_id = body
            .claude_session
            .as_ref()
            .map(|c| c.id.clone())
            .context("not a Claude session")?;
        let terminal = crate::terminal::create_from_body(terminals, self, body)?;
        let deadline = Instant::now() + TERMINAL_START;
        while Instant::now() < deadline {
            if let Some(session) = self.get(&session_id) {
                return Ok(session);
            }
            watch(&terminal.id);
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
        let owned = |s: &Arc<AgentSession>| s.mod_link().is_some_and(|l| l.has_token(token));
        self.get(session_id)
            .filter(owned)
            .or_else(|| lock(&self.inner).values().find(|s| owned(s)).cloned())
    }

    /// Fold lines a terminal's plugin posted into its session; numbered from
    /// `seq`, a line already folded (a retry, or a slow post's copy) is skipped.
    pub fn feed_mod(
        &self,
        session: &Arc<AgentSession>,
        lines: &[serde_json::Value],
        epoch: Option<&str>,
        seq: Option<u64>,
    ) {
        let mut let_go = false;
        let fold = |line: &serde_json::Value| {
            if let_go {
                return;
            }
            if let Some(link) = session.mod_link() {
                link.note_line(line);
                // The plugin gave up asking and the terminal asks it: wait there,
                // before the line's fold publishes the session's state.
                let handed = line.get("type").and_then(serde_json::Value::as_str)
                    == Some("control_cancel_request")
                    && line.get("workbench_in_terminal") == Some(&serde_json::Value::Bool(true));
                let id = line.get("request_id").and_then(serde_json::Value::as_str);
                if let Some(id) = id.filter(|id| handed && !link.fell_back(id)) {
                    link.fall_back(id, session.waiting_for(id));
                }
            }
            self.forget_replaced_pick(session, line);
            // Anything in the terminal can post a reset: it may move the chat
            // only to a well-formed id no other live session has.
            let mut refused = None;
            let applied = session.apply_line(&line.to_string(), |new_id, resumed| {
                if claude::validate(new_id).is_err() {
                    refused = Some(format!("The terminal moved to a session id Workbench can't use ({new_id:?})."));
                    return false;
                }
                if !session::rekey(&self.inner, session, new_id, resumed) {
                    refused = Some(format!("The terminal moved to session {new_id}, which another chat already has open."));
                    return false;
                }
                if let Some(link) = session.mod_link() {
                    self.bind_grant(&link.token, new_id);
                }
                true
            });
            if !applied {
                // The terminal's `claude` now runs another conversation: what
                // it posts no longer belongs in this chat, so the chat lets go.
                let why = refused.unwrap_or_default();
                tracing::warn!("refused a conversation reset: {why}");
                session.notice(format!(
                    "{why} This chat let go of the terminal; open the session there to continue."
                ));
                self.detach(session);
                let_go = true;
                return;
            }
            match line.get("type").and_then(serde_json::Value::as_str) {
                Some("result") => {
                    session.learn_cache_ttl();
                    session.recheck_goal();
                    // Asks the turn's end withdrew have nobody to answer.
                    if let Some(link) = session.mod_link() {
                        link.keep_asks(&session.pending_approvals());
                    }
                }
                Some("assistant") => session.learn_cache_ttl_early(),
                Some("workbench_goal_status") => {
                    if let Some(uuid) = line.get("uuid").and_then(serde_json::Value::as_str) {
                        session.learn_goal_status(uuid);
                    }
                }
                _ => {}
            }
        };
        match session.mod_link() {
            Some(link) => {
                link.touch();
                link.fold_new(epoch, seq, lines, fold);
            }
            None => lines.iter().for_each(fold),
        }
    }

    /// A model picked in the TUI (a `system:init` naming another pick) replaces
    /// the chat's, so a restart doesn't bring the chat's back; so does an effort
    /// picked there (the plugin says it dropped the chat's: `effortCleared`).
    fn forget_replaced_pick(&self, session: &AgentSession, line: &serde_json::Value) {
        if line["type"] != "system" || line["subtype"] != "init" {
            return;
        }
        let id = session.id();
        if let Some(choice) = line.get("modelChoice").and_then(serde_json::Value::as_str) {
            let mut picks = lock(&self.model_picks);
            if picks
                .get(&id)
                .is_some_and(|p| p["request"]["model"] != choice)
            {
                picks.remove(&id);
            }
        }
        if line["effortCleared"] == true {
            lock(&self.effort_picks).remove(&id);
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
            self.halt(session);
        }
        self.stop_halted(std::slice::from_ref(session));
    }

    /// Stop whatever chat sessions (either kind) a pane owns; a Claude chat's
    /// process is its terminal's `claude`, so that terminal goes too. Blocking.
    pub fn stop_pane(&self, pane_id: &str) -> usize {
        let owned = self.halt_matching(|s| s.pane_id.as_deref() == Some(pane_id));
        self.stop_halted(&owned);
        for session in &owned {
            self.kill_terminal(session);
        }
        owned.len()
    }

    /// A terminal is being closed: stop the chats it hosts first. Blocking.
    pub fn end_terminal(&self, terminal_id: &str) {
        let hosted = self.halt_matching(|s| {
            s.mod_link()
                .is_some_and(|l| l.terminal_id.as_deref() == Some(terminal_id))
        });
        self.stop_halted(&hosted);
    }

    /// [`Self::halt`] every session matching `keep`, under the lifecycle lock.
    fn halt_matching(&self, keep: impl Fn(&AgentSession) -> bool) -> Vec<Arc<AgentSession>> {
        let _lifecycle = lock(&self.lifecycle);
        let found = self.sessions(keep);
        for session in &found {
            self.halt(session);
        }
        found
    }

    fn kill_terminal(&self, session: &AgentSession) {
        let terminal = self.own_terminal(session);
        if let (Some(id), Some((terminals, _))) = (terminal, self.terminals.get()) {
            terminals.kill(&id);
        }
    }

    /// The server terminal a session's `claude` runs in and that the session
    /// owns. A desktop native pane's terminal is the person's shell: stopping
    /// the chat leaves it, only closing the pane ends it, and it can't be
    /// restarted into another terminal (the view shows only its own).
    pub fn own_terminal(&self, session: &AgentSession) -> Option<String> {
        let id = session.mod_link()?.terminal_id.clone()?;
        let native = self
            .terminals
            .get()
            .is_some_and(|(terminals, _)| terminals.is_native(&id));
        (!native).then_some(id)
    }

    /// Stop every session (the app is quitting or installing an update). Blocking.
    pub fn kill_all(&self) {
        let all = self.halt_matching(|_| true);
        lock(&self.inner).clear();
        self.attention.sessions_changed();
        self.stop_halted(&all);
    }

    /// Every live session, most recently changed first.
    pub fn summaries(&self) -> Vec<AgentSummary> {
        let mut grouped: Vec<(Arc<AgentSession>, Vec<String>)> = Vec::new();
        for (id, session) in lock(&self.inner).iter() {
            // A new Codex thread is listed once it has an id to attach to.
            if session::is_pending(id) {
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
    /// [`Self::stop_halted`] has stopped it. Under the lifecycle lock.
    fn halt(&self, session: &Arc<AgentSession>) {
        let mut ids: Vec<String> = lock(&self.inner)
            .iter()
            .filter(|(_, s)| Arc::ptr_eq(s, session))
            .map(|(id, _)| id.clone())
            .collect();
        let id = session.id();
        if !id.is_empty() && !ids.contains(&id) {
            ids.push(id);
        }
        self.forget(session);
        lock(&self.exiting).push((ids, session.clone()));
    }

    /// Stop sessions [`Self::halt`] unregistered, all at once and outside the
    /// lifecycle lock: each process gets a grace period to exit.
    fn stop_halted(&self, sessions: &[Arc<AgentSession>]) {
        let stop = |session: &Arc<AgentSession>| {
            session.shutdown();
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
    fn codex_terminal_notify_reaches_the_shared_feed_without_starting_a_chat() {
        let agents = AgentManager::default();
        let cursor = agents.attention.since(None).cursor;
        agents.codex_notified("pane", "thread", "/project");
        let batch = agents.attention.since(Some(&cursor));
        assert_eq!(batch.events.len(), 1);
        assert!(batch.events[0].terminal_only);
        assert_eq!(batch.events[0].session_id, "thread");
        assert!(agents.summaries().is_empty());
    }

    #[test]
    fn mod_env_hands_a_terminal_its_own_token_on_loopback() {
        let grant = || ModGrant {
            pane_id: Some("pane".into()),
            project_path: "/p".into(),
            worktree_path: None,
            claude_account_id: None,
            cwd: "/p".into(),
            resume_at: None,
            permission_mode: None,
            terminal_id: None,
            session_ids: Vec::new(),
        };
        let agents = AgentManager::default();
        assert!(
            agents.mod_env(grant()).unwrap().is_none(),
            "no listener yet"
        );
        agents.bind_terminals(crate::terminal::TerminalManager::default(), 4321);
        let (token, env) = agents.mod_env(grant()).unwrap().unwrap();
        assert_eq!(
            env[..2],
            [
                ("WORKBENCH_MOD_URL", "http://127.0.0.1:4321".to_string()),
                ("WORKBENCH_MOD_TOKEN", token.clone()),
            ]
        );
        let hook = env.iter().find(|(k, _)| *k == "WORKBENCH_HOOK_SOCKET");
        assert_eq!(
            hook.map(|(_, v)| v.clone()),
            agents.hooks.socket(),
            "the server's own bridge"
        );
        assert!(hook.is_some());
        assert!(lock(&agents.mod_grants).contains_key(&token));
        let (other, _) = agents.mod_env(grant()).unwrap().unwrap();
        assert_ne!(token, other, "each terminal gets its own");
    }

    #[tokio::test]
    async fn a_plugin_attaching_again_with_its_token_is_sent_its_picks_again() {
        let agents = AgentManager::default();
        let token = agents
            .grant_mod(ModGrant {
                pane_id: None,
                project_path: "/p".into(),
                worktree_path: None,
                claude_account_id: None,
                cwd: "/p".into(),
                resume_at: None,
                permission_mode: None,
                terminal_id: None,
                session_ids: Vec::new(),
            })
            .unwrap();
        let sid = "5e5e5e5e-0000-4000-8000-0000000000ad";
        let session = agents.attach_mod(&token, sid).unwrap();
        let link = session.mod_link().unwrap().clone();
        let wait = Duration::from_millis(20);
        let _initialize = link.take(wait, None).await;

        let pick = |subtype: &str, request: serde_json::Value| {
            let mut request = request;
            request["subtype"] = subtype.into();
            serde_json::json!({"type": "control_request", "request_id": "old", "request": request})
        };
        lock(&agents.model_picks).insert(
            sid.into(),
            pick("set_model", serde_json::json!({"model": "sonnet"})),
        );
        lock(&agents.effort_picks).insert(
            sid.into(),
            pick(
                "apply_flag_settings",
                serde_json::json!({"settings": {"effortLevel": "high"}}),
            ),
        );

        // Its worker restarted: the same token says hello again.
        let again = agents.attach_mod(&token, sid).unwrap();
        assert!(Arc::ptr_eq(&again, &session));
        let sent = link.take(wait, None).await;
        let subtypes: Vec<_> = sent
            .iter()
            .map(|l| l["request"]["subtype"].clone())
            .collect();
        assert_eq!(subtypes, ["set_model", "apply_flag_settings"]);
        assert!(sent.iter().all(|l| l["request_id"] != "old"));

        // Only an effort picked in the TUI replaces the chat's, not a request
        // that ran with another (one queued before the pick, a model without effort).
        let init = |effort: serde_json::Value| serde_json::json!({"type": "system", "subtype": "init", "effort": effort});
        agents.feed_mod(&session, &[init(serde_json::Value::Null)], None, None);
        agents.feed_mod(&session, &[init("low".into())], None, None);
        assert!(lock(&agents.effort_picks).contains_key(sid));
        let mut cleared = init("low".into());
        cleared["effortCleared"] = true.into();
        agents.feed_mod(&session, &[cleared], None, None);
        assert!(!lock(&agents.effort_picks).contains_key(sid));
    }
}
