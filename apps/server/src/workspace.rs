//! The workspace service: the server owns the one workspace → tab → pane
//! model (`workbench_core::workspace`) every client renders. Commands run
//! through `ops::apply` one at a time; their effects start and stop processes
//! through the terminal and chat managers (`exec`), and what those processes
//! do is folded back into the model and each pane's runtime state (`fold`).
//! Clients follow it as full snapshots (`routes`). The standalone server and
//! the desktop's embedded one run this same code. See `docs/WORKSPACE_MODEL.md`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{watch, Notify};
use workbench_core::claude_transcript::{RunningSummary, WaitingSummary};
use workbench_core::workspace::persist::{self, LocalState, WorkspacesFile};
use workbench_core::workspace::{ops, Command, Effect, Model, PaneKind};

use crate::agent::AgentManager;
use crate::terminal::TerminalManager;

mod exec;
mod fold;
mod lock;
pub mod routes;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    #[default]
    Starting,
    /// Claude Code asks to trust the folder before anything else runs.
    NeedsTrust,
    Running,
    /// The process ended without an End: the pane stays, to restart.
    Exited,
}

/// What a pane's process is doing; never saved.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneRuntime {
    pub terminal_id: Option<String>,
    pub status: Status,
    pub title: Option<String>,
    pub busy: bool,
    /// Unix ms the current turn started; null while idle.
    pub busy_since: Option<u64>,
    /// Unix ms the last turn went idle.
    pub turn_ended_at: Option<u64>,
    /// The tool call the current turn is running.
    pub running: Option<RunningSummary>,
    pub waiting: Option<WaitingSummary>,
    /// Unix ms the pane started waiting on its current request.
    pub waiting_since: Option<u64>,
    /// Why the last spawn failed.
    pub error: Option<String>,
    /// Its session ran (a Claude chat attached): gone again is an exit.
    #[serde(skip)]
    ran: bool,
}

struct State {
    model: Model,
    local: LocalState,
    runtime: HashMap<String, PaneRuntime>,
    /// Bumped by every change a snapshot may show.
    gen: u64,
}

/// A snapshot as clients get it. `rev` grows only when the snapshot differs
/// from the one before, so every subscriber sees the same `rev` sequence.
#[derive(Debug, Default)]
pub struct Published {
    pub rev: u64,
    /// The state change it includes, for a command to wait on.
    gen: u64,
    workspaces: String,
    pub json: String,
}

/// What a command opened or found, and the change it made.
#[derive(Debug, Default)]
pub struct Applied {
    pub gen: u64,
    pub workspace_id: Option<String>,
    pub tab_id: Option<String>,
    pub pane_id: Option<String>,
}

struct Ctx {
    terminals: TerminalManager,
    agents: AgentManager,
    jobs: Mutex<mpsc::Sender<exec::Job>>,
}

struct Inner {
    /// Loads and saves the config dir's model file (the desktop and the
    /// standalone binary); otherwise in memory only (tests, hand-built state).
    persistent: bool,
    dir: OnceLock<PathBuf>,
    state: Mutex<State>,
    published: watch::Sender<Arc<Published>>,
    dirty: Notify,
    ctx: OnceLock<Ctx>,
    /// Held while this process keeps the config dir's model file.
    lock: OnceLock<lock::ModelLock>,
}

#[derive(Clone)]
pub struct WorkspaceService(Arc<Inner>);

impl Default for WorkspaceService {
    fn default() -> Self {
        Self::new(false)
    }
}

impl WorkspaceService {
    fn new(persistent: bool) -> Self {
        Self(Arc::new(Inner {
            persistent,
            dir: OnceLock::new(),
            state: Mutex::new(State {
                model: Model::default(),
                local: LocalState::default(),
                runtime: HashMap::new(),
                gen: 0,
            }),
            published: watch::channel(Arc::new(Published::default())).0,
            dirty: Notify::new(),
            ctx: OnceLock::new(),
            lock: OnceLock::new(),
        }))
    }

    /// A service kept in the config dir's `workspaces.v2.json` (read at boot).
    pub fn persistent() -> Self {
        Self::new(true)
    }

    /// Once, when the first listener is up (terminal plugins reach it): load
    /// the saved model and start every pane's process. Later calls do nothing.
    pub fn boot(&self, terminals: TerminalManager, agents: AgentManager) {
        let (tx, rx) = mpsc::channel();
        let ctx = Ctx {
            terminals,
            agents,
            jobs: Mutex::new(tx),
        };
        if self.0.ctx.set(ctx).is_err() {
            return;
        }
        exec::start(self.clone(), rx);
        let dir = workbench_core::paths::workbench_config_dir();
        let held = self.0.persistent.then(|| lock::ModelLock::take(&dir));
        if let Some(None) = held {
            tracing::warn!(
                "another Workbench process keeps the workspace model in {}; this one runs without saving it",
                dir.display()
            );
        }
        if let Some(Some(held)) = held {
            let _ = self.0.lock.set(held);
            // Until the desktop renders this model (Phase 3), its own
            // `workspaces.json` describes panes it runs itself: only a saved v2
            // file is booted, or each of those would run twice.
            if dir.join(persist::FILE).exists() {
                match persist::load(&dir) {
                    Ok(file) => {
                        let mut state = lock(&self.0.state);
                        state.model = file.model;
                        state.local = file.local;
                    }
                    Err(e) => tracing::error!("workspace model not loaded: {e:#}"),
                }
            }
            let _ = self.0.dir.set(dir);
        }
        let effects = {
            let mut state = lock(&self.0.state);
            ops::boot(&mut state.model)
        };
        self.run(effects);
        fold::start(self.clone());
    }

    fn ctx(&self) -> Option<&Ctx> {
        self.0.ctx.get()
    }

    /// Apply a client's command. Blocking (it may save the model).
    pub fn command(&self, cmd: Command) -> Result<Applied> {
        let cmd = self.with_defaults(cmd)?;
        let effects = {
            let mut state = lock(&self.0.state);
            ops::apply(&mut state.model, cmd)?
        };
        let mut applied = self.run(effects);
        if self.ctx().is_none() {
            self.publish();
        }
        applied.gen = lock(&self.0.state).gen;
        Ok(applied)
    }

    /// Apply a change the server saw a process make. `Err`: the model refused
    /// it (another pane holds that session).
    fn fold_in(&self, cmd: Command) -> Result<()> {
        let effects = {
            let mut state = lock(&self.0.state);
            ops::apply(&mut state.model, cmd)?
        };
        self.run(effects);
        Ok(())
    }

    /// A Claude NewSession's account, decided as every launch's is
    /// (`claude_accounts::for_launch`): a pick (`""` the default login), else
    /// the login holding a resumed transcript, the project's own, the active one.
    fn with_defaults(&self, cmd: Command) -> Result<Command> {
        let Command::NewSession {
            target,
            kind: PaneKind::Claude,
            resume,
            prompt,
            account_id,
            label,
            command,
            codex_mode,
        } = cmd
        else {
            return Ok(cmd);
        };
        let project_path = match &target {
            workbench_core::workspace::Target::Location { project_path, .. } => {
                project_path.clone()
            }
            workbench_core::workspace::Target::Workspace { workspace_id } => lock(&self.0.state)
                .model
                .workspace(workspace_id)
                .map(|w| w.project_path.clone())
                .unwrap_or_default(),
        };
        // A new session's id is minted by the model; any fresh one has no transcript.
        let session = resume.clone().unwrap_or_else(ops::new_id);
        let account_id = workbench_core::claude_accounts::for_launch_saved(
            account_id.as_deref(),
            &project_path,
            Some(&session),
        )?;
        Ok(Command::NewSession {
            target,
            kind: PaneKind::Claude,
            resume,
            prompt,
            account_id,
            label,
            command,
            codex_mode,
        })
    }

    /// Record what `effects` changed and queue the processes they start and stop.
    fn run(&self, effects: Vec<Effect>) -> Applied {
        let mut applied = Applied::default();
        let mut state = lock(&self.0.state);
        let mut job = Vec::new();
        for effect in effects {
            match &effect {
                Effect::Opened {
                    workspace_id,
                    tab_id,
                    pane_id,
                } => {
                    applied.workspace_id = Some(workspace_id.clone());
                    applied.tab_id = tab_id.clone();
                    applied.pane_id = pane_id.clone();
                }
                Effect::Persist => self.save(&state),
                Effect::SpawnShell { pane_id, .. }
                | Effect::SpawnClaude { pane_id, .. }
                | Effect::SpawnCodex { pane_id, .. } => {
                    let pane = exec::PaneCtx::of(&state.model, pane_id);
                    state
                        .runtime
                        .insert(pane_id.clone(), PaneRuntime::default());
                    job.push((effect, pane));
                }
                Effect::Stop { pane_id, .. }
                | Effect::End { pane_id, .. }
                | Effect::TrustFolder { pane_id, .. } => {
                    let mut pane = exec::PaneCtx::of(&state.model, pane_id);
                    pane.terminal_id = state
                        .runtime
                        .get(pane_id)
                        .and_then(|r| r.terminal_id.clone());
                    if matches!(effect, Effect::End { .. }) {
                        state.runtime.remove(pane_id);
                    }
                    job.push((effect, pane));
                }
            }
        }
        state.gen += 1;
        drop(state);
        if let (false, Some(ctx)) = (job.is_empty(), self.ctx()) {
            let _ = lock(&ctx.jobs).send(job);
        }
        self.0.dirty.notify_one();
        applied
    }

    fn save(&self, state: &State) {
        let Some(dir) = self.0.dir.get() else {
            return;
        };
        let file = WorkspacesFile {
            model: state.model.clone(),
            local: state.local.clone(),
        };
        if let Err(e) = persist::save(dir, &file) {
            tracing::error!("workspace model not saved: {e:#}");
        }
    }

    /// Change one pane's runtime state (an exec result).
    fn update(&self, pane_id: &str, f: impl FnOnce(&mut PaneRuntime)) {
        let mut state = lock(&self.0.state);
        if state.model.pane(pane_id).is_none() {
            return;
        }
        f(state.runtime.entry(pane_id.to_string()).or_default());
        state.gen += 1;
        drop(state);
        self.0.dirty.notify_one();
    }

    /// An End that came by an older route (a terminal or a chat closed by
    /// id): the pane goes too, so every client sees it close. Blocking.
    pub fn close_pane(&self, pane_id: Option<String>) {
        if let Some(pane_id) = pane_id {
            if let Err(e) = self.command(Command::ClosePane { pane_id }) {
                tracing::warn!("closing a pane: {e:#}");
            }
        }
    }

    /// The pane running in terminal `id`, if any.
    pub fn pane_for_terminal(&self, id: &str) -> Option<String> {
        lock(&self.0.state)
            .runtime
            .iter()
            .find(|(_, r)| r.terminal_id.as_deref() == Some(id))
            .map(|(pane, _)| pane.clone())
    }

    /// The pane holding session `id` (either kind), if any.
    pub fn pane_for_session(&self, id: &str) -> Option<String> {
        let state = lock(&self.0.state);
        [PaneKind::Claude, PaneKind::Codex]
            .into_iter()
            .find_map(|kind| state.model.pane_for_session(kind, id))
            .map(|p| p.id.clone())
    }

    /// Follow the snapshots; the first is refreshed for the new watcher.
    pub fn subscribe(&self) -> watch::Receiver<Arc<Published>> {
        let rx = self.0.published.subscribe();
        self.0.dirty.notify_one();
        rx
    }

    /// Build the snapshot and publish it: a new `rev` only when it changed.
    fn publish(&self) {
        let (gen, workspaces) = {
            let state = lock(&self.0.state);
            (state.gen, snapshot(&state))
        };
        self.0.published.send_modify(|last| {
            let rev = if last.workspaces == workspaces {
                last.rev
            } else {
                last.rev + 1
            };
            let json = format!(r#"{{"rev":{rev},"workspaces":{workspaces}}}"#);
            *last = Arc::new(Published {
                rev,
                gen,
                workspaces,
                json,
            });
        });
    }
}

/// The model's workspaces with each pane's runtime state merged in.
fn snapshot(state: &State) -> String {
    let mut workspaces = serde_json::to_value(&state.model.workspaces).unwrap_or_default();
    let panes = workspaces
        .as_array_mut()
        .into_iter()
        .flatten()
        .filter_map(|w| w["tabs"].as_array_mut())
        .flatten()
        .filter_map(|t| t["panes"].as_array_mut())
        .flatten();
    for pane in panes {
        let id = pane["id"].as_str().unwrap_or_default();
        let runtime = state.runtime.get(id).cloned().unwrap_or_default();
        if let (Value::Object(pane), Ok(Value::Object(runtime))) =
            (pane, serde_json::to_value(runtime))
        {
            pane.extend(runtime);
        }
    }
    workspaces.to_string()
}
