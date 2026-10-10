//! The workspace service: the server owns the one workspace → tab → pane
//! model (`workbench_core::workspace`) every client renders. Commands run
//! through `ops::apply` one at a time; their effects start and stop processes
//! through the terminal and chat managers (`exec`), and what those processes
//! do is folded back into the model and each pane's runtime state (`fold`).
//! Clients follow it as full snapshots (`routes`). The standalone server and
//! the desktop's embedded one run this same code. See `docs/WORKSPACE_MODEL.md`.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{watch, Notify};
use workbench_core::claude_transcript::{RunningSummary, RunningTasks, WaitingSummary};
use workbench_core::workspace::persist::{self, LocalState, WorkspacesFile};
use workbench_core::workspace::{ops, Command, Effect, Model, PaneKind};

use crate::agent::AgentManager;
use crate::terminal::TerminalManager;

mod exec;
mod fold;
mod load;
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
    /// Subagents and background tasks still going.
    pub running_tasks: RunningTasks,
    pub waiting: Option<WaitingSummary>,
    /// Unix ms the pane started waiting on its current request.
    pub waiting_since: Option<u64>,
    /// Why the last spawn failed.
    pub error: Option<String>,
    /// Something to tell the person about how the current spawn started;
    /// cleared by the next spawn.
    pub notice: Option<String>,
    /// Bumped by every spawn (start, restart, mode switch), so a client knows
    /// to re-attach even when it never saw the pane stop.
    pub generation: u64,
    /// Its session ran (a Claude chat attached): gone again is an exit.
    #[serde(skip)]
    ran: bool,
}

struct State {
    model: Model,
    local: LocalState,
    persistence: load::Persistence,
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
#[derive(Debug, Default, Clone)]
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
    /// Recent commands by their client `requestId`, so a retry gets the first
    /// answer instead of a second session.
    requests: Mutex<VecDeque<(String, Instant, Applied)>>,
    saving: Mutex<Saving>,
}

/// How many request ids, and for how long, a command's answer is kept.
const REQUESTS_KEPT: usize = 256;
const REQUEST_TTL: Duration = Duration::from_secs(300);

/// The model file's writes ([`WorkspaceService::save`]).
#[derive(Default)]
struct Saving {
    /// The newest model handed in and not yet written, with its generation.
    next: Option<(u64, WorkspacesFile)>,
    /// A caller is writing; it takes `next` when done.
    writing: bool,
    /// The generation last written.
    written: u64,
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
                persistence: load::Persistence::default(),
                runtime: HashMap::new(),
                gen: 0,
            }),
            published: watch::channel(Arc::new(Published::default())).0,
            dirty: Notify::new(),
            ctx: OnceLock::new(),
            lock: OnceLock::new(),
            requests: Mutex::new(VecDeque::new()),
            saving: Mutex::default(),
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
            lock(&self.0.state).persistence =
                load::Persistence::locked(&dir, lock::ModelLock::holder_of(&dir));
        }
        if let Some(Some(held)) = held {
            let _ = self.0.lock.set(held);
            // A model that can't be read is never saved over.
            match load::load(&dir) {
                Ok(file) => {
                    let mut state = lock(&self.0.state);
                    state.model = file.model;
                    state.local = file.local;
                    drop(state);
                    let _ = self.0.dir.set(dir);
                }
                Err(failed) => lock(&self.0.state).persistence = failed,
            }
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

    /// Apply a client's command once per `request_id`: a retry of one already
    /// applied gets its answer again. Blocking.
    pub fn command_once(&self, request_id: Option<String>, cmd: Command) -> Result<Applied> {
        let Some(request_id) = request_id else {
            return self.command(cmd);
        };
        // Held across the apply, so a retry racing the first waits for its answer.
        let mut requests = lock(&self.0.requests);
        requests.retain(|(_, at, _)| at.elapsed() < REQUEST_TTL);
        if let Some((_, _, applied)) = requests.iter().find(|(id, ..)| *id == request_id) {
            return Ok(applied.clone());
        }
        let applied = self.command(cmd)?;
        if requests.len() >= REQUESTS_KEPT {
            requests.pop_front();
        }
        requests.push_back((request_id, Instant::now(), applied.clone()));
        Ok(applied)
    }

    /// Apply a client's command. Blocking (it may save the model).
    pub fn command(&self, cmd: Command) -> Result<Applied> {
        self.follow_rekey(&cmd);
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

    /// A resume of a session `/clear` re-keyed: fold the re-key in now rather
    /// than at the next fold tick, so the model finds the pane that holds it
    /// (by its new id or an old one) instead of opening a second.
    fn follow_rekey(&self, cmd: &Command) {
        let (
            Command::NewSession {
                resume: Some(id), ..
            },
            Some(ctx),
        ) = (cmd, self.ctx())
        else {
            return;
        };
        let live = ctx
            .agents
            .summaries()
            .into_iter()
            .find(|s| s.session_id == *id || s.previous_ids.contains(id));
        if let Some(s) = live.filter(|s| !s.previous_ids.is_empty()) {
            let _ = self.fold_in(Command::SessionRekeyed {
                session_id: s.session_id,
                previous_ids: s.previous_ids,
            });
        }
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
        let mut persist = false;
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
                Effect::Persist => persist = true,
                Effect::SpawnShell { pane_id, .. }
                | Effect::SpawnClaude { pane_id, .. }
                | Effect::SpawnCodex { pane_id, .. } => {
                    let pane = exec::PaneCtx::of(&state.model, pane_id);
                    let generation = state.runtime.get(pane_id).map_or(0, |r| r.generation) + 1;
                    state.runtime.insert(
                        pane_id.clone(),
                        PaneRuntime {
                            generation,
                            ..PaneRuntime::default()
                        },
                    );
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
        let save = persist.then(|| {
            let file = WorkspacesFile {
                model: state.model.clone(),
                local: state.local.clone(),
            };
            (state.gen, file)
        });
        drop(state);
        if let Some((gen, file)) = save {
            self.save(gen, file);
        }
        if let (false, Some(ctx)) = (job.is_empty(), self.ctx()) {
            let _ = lock(&ctx.jobs).send(job);
        }
        self.0.dirty.notify_one();
        applied
    }

    /// Write `file`, the model at `gen`, outside the state lock: a slow disk
    /// must not hold up every reader of the model (the publish loop takes it
    /// on an async worker). One caller writes at a time, and always the newest
    /// model handed in: the others leave theirs and return at once, and an
    /// older model never lands after a newer one.
    fn save(&self, gen: u64, file: WorkspacesFile) {
        let Some(dir) = self.0.dir.get() else {
            return;
        };
        {
            let mut saving = lock(&self.0.saving);
            let queued = saving.next.as_ref().map_or(0, |(g, _)| *g);
            if gen <= saving.written || gen <= queued {
                return;
            }
            saving.next = Some((gen, file));
            if saving.writing {
                return;
            }
            saving.writing = true;
        }
        loop {
            let (gen, file) = {
                let mut saving = lock(&self.0.saving);
                let Some(next) = saving.next.take() else {
                    saving.writing = false;
                    return;
                };
                next
            };
            if let Err(e) = persist::save(dir, &file) {
                tracing::error!("workspace model not saved: {e:#}");
            }
            lock(&self.0.saving).written = gen;
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
            let json = format!(r#"{{"rev":{rev},{workspaces}}}"#);
            *last = Arc::new(Published {
                rev,
                gen,
                workspaces,
                json,
            });
        });
    }
}

/// A snapshot's fields after `rev`: the model's workspaces with each pane's
/// runtime state merged in, whether it's saved, and the desktop's saved
/// per-device state (`local`) for it to take over once.
fn snapshot(state: &State) -> String {
    let local = serde_json::json!({
        "selectedId": state.local.selected_id,
        "activeTabIds": state.local.active_tab_ids,
        "chatPanes": state.local.chat_panes,
    });
    let persistence = serde_json::to_string(&state.persistence).unwrap_or_default();
    format!(
        r#""workspaces":{},"persistence":{persistence},"local":{local}"#,
        workspaces(state)
    )
}

fn workspaces(state: &State) -> String {
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
