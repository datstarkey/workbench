//! Carries out the model's effects, in order, on one thread: a stop or end
//! always lands before the spawn that replaces it. Spawns only start the
//! process; `fold` sees it attach or exit.

use std::sync::mpsc;
use std::time::Duration;

use workbench_core::types::ProjectConfig;
use workbench_core::workspace::{model::same_path, CodexMode, Command, Effect, Model, Renderer};

use super::{Status, WorkspaceService};
use crate::agent::{Launch, StartAgent};
use crate::terminal::{
    create_from_body, ClaudeSessionLaunch, CodexSessionLaunch, CreateTerminalBody,
};

pub(super) type Job = Vec<(Effect, PaneCtx)>;

/// Where a pane runs, read from the model when its effect was queued.
#[derive(Debug, Default)]
pub(super) struct PaneCtx {
    project_path: String,
    worktree_path: Option<String>,
    label: Option<String>,
    pub(super) terminal_id: Option<String>,
}

impl PaneCtx {
    pub(super) fn of(model: &Model, pane_id: &str) -> Self {
        let Some((w, t, _)) = model.pane_at(pane_id) else {
            return Self::default();
        };
        let ws = &model.workspaces[w];
        Self {
            project_path: ws.project_path.clone(),
            worktree_path: ws.worktree_path.clone(),
            label: Some(ws.tabs[t].label.clone()),
            terminal_id: None,
        }
    }
}

/// The registered project at `path`.
pub(super) fn project(path: String) -> Option<ProjectConfig> {
    workbench_core::config::load_projects()
        .ok()?
        .into_iter()
        .find(|p| same_path(&p.path, &path))
}

pub(super) fn start(service: WorkspaceService, jobs: mpsc::Receiver<Job>) {
    std::thread::spawn(move || {
        for job in jobs {
            for (effect, pane) in job {
                run(&service, effect, pane);
            }
        }
    });
}

fn run(service: &WorkspaceService, effect: Effect, pane: PaneCtx) {
    let Some(ctx) = service.ctx() else {
        return;
    };
    let (terminals, agents) = (&ctx.terminals, &ctx.agents);
    match effect {
        Effect::SpawnShell {
            pane_id,
            renderer,
            command,
            account_id,
            ..
        } => {
            let mut body = terminal_body(&pane, &pane_id, renderer, account_id);
            body.command = command;
            spawn_terminal(service, &pane_id, body, Status::Running);
        }
        Effect::SpawnClaude {
            pane_id,
            renderer,
            session_id,
            resume,
            account_id,
            prompt,
            ..
        } => {
            let mut body = terminal_body(&pane, &pane_id, renderer, account_id);
            body.claude_session = Some(ClaudeSessionLaunch {
                id: session_id,
                resume,
                prompt,
                ..Default::default()
            });
            // Running once its plugin attaches (`fold`).
            spawn_terminal(service, &pane_id, body, Status::Starting);
        }
        Effect::SpawnCodex {
            pane_id,
            renderer,
            session_id,
            mode: CodexMode::Tui,
            prompt,
            ..
        } => {
            let mut body = terminal_body(&pane, &pane_id, renderer, None);
            body.codex_session = Some(CodexSessionLaunch {
                id: session_id,
                prompt,
            });
            spawn_terminal(service, &pane_id, body, Status::Running);
        }
        Effect::SpawnCodex {
            pane_id,
            session_id,
            mode: CodexMode::AppServer,
            prompt,
            ..
        } => {
            // Waits for codex to open its thread: off this thread, so other
            // panes' effects don't queue behind it.
            let service = service.clone();
            std::thread::spawn(move || {
                if let Err(e) = start_codex_chat(&service, &pane_id, &pane, session_id, prompt) {
                    service.update(&pane_id, |rt| failed(rt, &e));
                }
            });
        }
        Effect::Stop { pane_id, .. } => {
            // One process per session file: the old one is gone before a respawn.
            if let Some(id) = &pane.terminal_id {
                terminals.kill_and_wait(id);
            }
            agents.stop_pane(&pane_id, false);
        }
        Effect::End { pane_id, .. } => {
            if let Some(id) = &pane.terminal_id {
                agents.end_terminal(id);
                terminals.kill(id);
            }
            agents.stop_pane(&pane_id, true);
        }
        Effect::TrustFolder { .. } => {
            let Some(id) = &pane.terminal_id else {
                return;
            };
            for keys in workbench_core::claude_launch::TRUST_ACCEPT_KEYS {
                terminals.type_keys(id, keys);
                std::thread::sleep(Duration::from_millis(300));
            }
        }
        Effect::Opened { .. } | Effect::Persist => {}
    }
}

fn terminal_body(
    pane: &PaneCtx,
    pane_id: &str,
    renderer: Renderer,
    account_id: Option<String>,
) -> CreateTerminalBody {
    CreateTerminalBody {
        project_path: pane.project_path.clone(),
        worktree_path: pane.worktree_path.clone(),
        name: pane.label.clone(),
        command: None,
        claude_session: None,
        codex_session: None,
        cols: 120,
        rows: 40,
        pane_id: Some(pane_id.to_string()),
        // The project's configured shell, never one a client names.
        shell: project(pane.project_path.clone()).and_then(|p| p.shell),
        claude_account_id: account_id,
        native: renderer == Renderer::Native,
    }
}

fn spawn_terminal(
    service: &WorkspaceService,
    pane_id: &str,
    body: CreateTerminalBody,
    status: Status,
) {
    let Some(ctx) = service.ctx() else {
        return;
    };
    match create_from_body(&ctx.terminals, &ctx.agents, body) {
        Ok(meta) => {
            // Closed while it started: nobody will end it.
            if lock_has_pane(service, pane_id) {
                service.update(pane_id, |rt| {
                    rt.terminal_id = Some(meta.id);
                    rt.status = status;
                });
            } else {
                ctx.agents.end_terminal(&meta.id);
                ctx.terminals.kill(&meta.id);
            }
        }
        Err(e) => service.update(pane_id, |rt| failed(rt, &e)),
    }
}

fn lock_has_pane(service: &WorkspaceService, pane_id: &str) -> bool {
    super::lock(&service.0.state).model.pane(pane_id).is_some()
}

fn failed(rt: &mut super::PaneRuntime, e: &anyhow::Error) {
    rt.status = Status::Exited;
    rt.error = Some(format!("{e:#}"));
}

fn start_codex_chat(
    service: &WorkspaceService,
    pane_id: &str,
    pane: &PaneCtx,
    session_id: Option<String>,
    prompt: Option<String>,
) -> anyhow::Result<()> {
    let ctx = service.ctx().ok_or_else(|| anyhow::anyhow!("not booted"))?;
    let cwd = crate::cwd::resolve_cwd(&pane.project_path, pane.worktree_path.as_deref())?;
    let settings = workbench_core::config::load_workbench_settings()?;
    let options = workbench_core::codex_controls::LaunchOptions::default().with_defaults(
        &settings.codex_approval_policy,
        &settings.codex_sandbox_mode,
    );
    let session = ctx.agents.start(StartAgent {
        cwd,
        project_path: pane.project_path.clone(),
        worktree_path: pane.worktree_path.clone(),
        pane_id: Some(pane_id.to_string()),
        hook_socket: None,
        claude_account_id: None,
        launch: Launch::Codex {
            // A thread with nothing on disk starts fresh; its new id is folded in below.
            thread_id: session_id.filter(|id| workbench_core::codex_launch::thread_exists(id)),
            mode: None,
            options,
        },
    })?;
    service.fold_in(Command::SessionAttached {
        pane_id: pane_id.to_string(),
        session_id: session.id(),
    })?;
    if let Some(prompt) = prompt {
        session.prompt(&prompt, &[], &[])?;
    }
    Ok(())
}
