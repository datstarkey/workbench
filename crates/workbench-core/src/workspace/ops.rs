//! Every rule that changes the workspace model. `apply` is pure: it edits the
//! model and returns the effects (processes to start or stop, a save) for the
//! caller to carry out, in order.

use anyhow::{bail, Context, Result};
use uuid::Uuid;

use super::command::{Command, Effect};
use super::model::{Model, Pane, PaneKind, Renderer, Workspace};

mod layout;
mod lifecycle;

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn apply(model: &mut Model, cmd: Command) -> Result<Vec<Effect>> {
    match cmd {
        Command::OpenWorkspace {
            project_path,
            project_name,
            worktree_path,
            branch,
            renderer,
        } => Ok(lifecycle::open_workspace(
            model,
            project_path,
            project_name,
            worktree_path,
            branch,
            renderer,
            false,
        )
        .1),
        Command::CloseWorkspace { workspace_id } => {
            let i = workspace_index(model, &workspace_id)?;
            let ws = model.workspaces.remove(i);
            Ok(ended(ws.tabs.iter().flat_map(|t| &t.panes)))
        }
        Command::CloseProject { project_path } => {
            Ok(lifecycle::close_project(model, &project_path))
        }
        Command::NewSession {
            target,
            kind,
            resume,
            prompt,
            account_id,
            label,
            command,
            codex_mode,
        } => lifecycle::new_session(
            model,
            target,
            lifecycle::NewPane {
                kind,
                resume,
                prompt,
                account_id,
                label,
                command,
                codex_mode,
            },
        ),
        Command::ClosePane { pane_id } => lifecycle::close_pane(model, &pane_id),
        Command::CloseTab { tab_id } => lifecycle::close_tab(model, &tab_id),
        Command::Restart { tab_id } => lifecycle::restart(model, &tab_id),
        Command::SetCodexMode { pane_id, mode } => lifecycle::set_codex_mode(model, &pane_id, mode),
        Command::Rename { tab_id, label } => layout::rename(model, &tab_id, &label),
        Command::Split { tab_id, direction } => layout::split(model, &tab_id, direction),
        Command::MovePane { pane_id, tab_id } => layout::move_pane(model, &pane_id, &tab_id),
        Command::MoveTab { tab_id, to_tab_id } => layout::move_tab(model, &tab_id, &to_tab_id),
        Command::MoveWorkspace {
            workspace_id,
            to_workspace_id,
        } => layout::move_workspace(model, &workspace_id, &to_workspace_id),
        Command::TrustFolder { pane_id } => {
            let (w, t, p) = pane_index(model, &pane_id)?;
            let ws = &model.workspaces[w];
            if ws.tabs[t].panes[p].kind != PaneKind::Claude {
                bail!("Only a Claude pane asks to trust its folder");
            }
            Ok(vec![Effect::TrustFolder {
                pane_id,
                cwd: ws.cwd().to_string(),
            }])
        }
        Command::UpdateProject {
            project_path,
            new_path,
            project_name,
        } => Ok(lifecycle::update_project(
            model,
            &project_path,
            &new_path,
            &project_name,
        )),
        Command::AccountMoved {
            pane_id,
            account_id,
        } => Ok(lifecycle::account_moved(model, &pane_id, account_id)),
        Command::SessionAttached {
            pane_id,
            session_id,
        } => lifecycle::attach(model, &pane_id, session_id),
        Command::SessionRekeyed {
            session_id,
            previous_ids,
        } => Ok(lifecycle::rekey(model, session_id, previous_ids)),
    }
}

/// The effects that start every pane's process, as a host does when it boots
/// a saved model. A session may already exist, so each is a resume.
pub fn boot(model: &mut Model) -> Vec<Effect> {
    let mut effects = Vec::new();
    let mut consumed = false;
    for w in 0..model.workspaces.len() {
        for t in 0..model.workspaces[w].tabs.len() {
            for p in 0..model.workspaces[w].tabs[t].panes.len() {
                let resume = model.workspaces[w].tabs[t].panes[p].session_id.is_some();
                let (effect, took) = spawn_at(model, (w, t, p), resume);
                effects.push(effect);
                consumed |= took;
            }
        }
    }
    if consumed {
        effects.push(Effect::Persist);
    }
    effects
}

/// The effect that starts a pane's process. A prompt is sent once, so it
/// leaves the pane here; the flag says whether one did (the model changed).
fn spawn(cwd: &str, renderer: Renderer, pane: &mut Pane, resume: bool) -> (Effect, bool) {
    let prompt = pane.prompt.take();
    let consumed = prompt.is_some();
    let pane_id = pane.id.clone();
    let cwd = cwd.to_string();
    let effect = match pane.kind {
        PaneKind::Shell => Effect::SpawnShell {
            pane_id,
            cwd,
            renderer,
            command: pane.command.clone(),
            account_id: pane.account_id.clone(),
        },
        PaneKind::Claude => Effect::SpawnClaude {
            pane_id,
            cwd,
            renderer,
            session_id: pane.session_id.clone().unwrap_or_default(),
            resume,
            account_id: pane.account_id.clone(),
            prompt,
        },
        PaneKind::Codex => Effect::SpawnCodex {
            pane_id,
            cwd,
            renderer,
            session_id: pane.session_id.clone(),
            mode: pane.codex_mode.unwrap_or_default(),
            // A thread that exists already started with its prompt.
            prompt: prompt.filter(|_| pane.session_id.is_none()),
        },
    };
    (effect, consumed)
}

/// `spawn` for the pane at `(w, t, p)`.
fn spawn_at(model: &mut Model, (w, t, p): (usize, usize, usize), resume: bool) -> (Effect, bool) {
    let ws = &mut model.workspaces[w];
    let cwd = ws.cwd().to_string();
    let renderer = ws.renderer;
    spawn(&cwd, renderer, &mut ws.tabs[t].panes[p], resume)
}

fn ended<'a>(panes: impl IntoIterator<Item = &'a Pane>) -> Vec<Effect> {
    panes
        .into_iter()
        .map(|p| Effect::End {
            pane_id: p.id.clone(),
            session_id: p.session_id.clone(),
        })
        .chain([Effect::Persist])
        .collect()
}

fn drop_split_with(ws: &mut Workspace, tab_id: &str) {
    if ws
        .split_view
        .as_ref()
        .is_some_and(|s| s.tab_ids.iter().any(|id| id == tab_id))
    {
        ws.split_view = None;
    }
}

fn workspace_index(model: &Model, id: &str) -> Result<usize> {
    model
        .workspaces
        .iter()
        .position(|w| w.id == id)
        .with_context(|| format!("No workspace {id}"))
}

fn tab_index(model: &Model, id: &str) -> Result<(usize, usize)> {
    model.tab_at(id).with_context(|| format!("No tab {id}"))
}

fn pane_index(model: &Model, id: &str) -> Result<(usize, usize, usize)> {
    model.pane_at(id).with_context(|| format!("No pane {id}"))
}

#[cfg(test)]
mod tests;
