//! Every rule that changes the workspace model. `apply` is pure: it edits the
//! model and returns the effects (processes to start or stop, a save) for the
//! caller to carry out, in order.

use anyhow::{bail, Context, Result};
use uuid::Uuid;

use super::command::{Command, Effect, Target};
use super::model::{
    CodexMode, Model, Pane, PaneKind, Renderer, SplitDirection, SplitView, Tab, Workspace,
};

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
        } => Ok(open_workspace(
            model,
            project_path,
            project_name,
            worktree_path,
            branch,
            renderer,
            false,
        )),
        Command::CloseWorkspace { workspace_id } => {
            let i = workspace_index(model, &workspace_id)?;
            let ws = model.workspaces.remove(i);
            Ok(ended(ws.tabs.iter().flat_map(|t| &t.panes)))
        }
        Command::CloseProject { project_path } => {
            let (closing, kept) = std::mem::take(&mut model.workspaces)
                .into_iter()
                .partition::<Vec<_>, _>(|w| w.project_path == project_path);
            model.workspaces = kept;
            if closing.is_empty() {
                return Ok(vec![]);
            }
            Ok(ended(
                closing.iter().flat_map(|w| &w.tabs).flat_map(|t| &t.panes),
            ))
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
        } => new_session(
            model,
            target,
            NewPane {
                kind,
                resume,
                prompt,
                account_id,
                label,
                command,
                codex_mode,
            },
        ),
        Command::ClosePane { pane_id } => {
            let (w, t, p) = pane_index(model, &pane_id)?;
            let tab = &mut model.workspaces[w].tabs[t];
            if tab.panes.len() > 1 {
                let pane = tab.panes.remove(p);
                return Ok(ended([&pane]));
            }
            let tab_id = tab.id.clone();
            close_tab(model, &tab_id)
        }
        Command::CloseTab { tab_id } => close_tab(model, &tab_id),
        Command::Restart { tab_id } => restart(model, &tab_id),
        Command::SetCodexMode { pane_id, mode } => set_codex_mode(model, &pane_id, mode),
        Command::Rename { tab_id, label } => {
            let label = label.trim();
            if label.is_empty() {
                bail!("A tab label can't be empty");
            }
            let (w, t) = tab_index(model, &tab_id)?;
            let tab = &mut model.workspaces[w].tabs[t];
            if tab.label == label {
                return Ok(vec![]);
            }
            tab.label = label.to_string();
            Ok(vec![Effect::Persist])
        }
        Command::Split { tab_id, direction } => split(model, &tab_id, direction),
        Command::MovePane { pane_id, tab_id } => move_pane(model, &pane_id, &tab_id),
        Command::MoveTab { tab_id, to_tab_id } => {
            let (w, from) = tab_index(model, &tab_id)?;
            let (to_w, to) = tab_index(model, &to_tab_id)?;
            if w != to_w {
                bail!("Tabs only move within their workspace");
            }
            Ok(move_item(&mut model.workspaces[w].tabs, from, to))
        }
        Command::MoveWorkspace {
            workspace_id,
            to_workspace_id,
        } => {
            let from = workspace_index(model, &workspace_id)?;
            let to = workspace_index(model, &to_workspace_id)?;
            Ok(move_item(&mut model.workspaces, from, to))
        }
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
        Command::SessionAttached {
            pane_id,
            session_id,
        } => {
            // Folded-in events can race a close: an unknown pane is a no-op.
            let Some((w, t, p)) = model.pane_at(&pane_id) else {
                return Ok(vec![]);
            };
            let pane = &mut model.workspaces[w].tabs[t].panes[p];
            if pane.session_id.as_deref() == Some(session_id.as_str()) {
                return Ok(vec![]);
            }
            // Another conversation: the old one's `/clear` ids no longer apply.
            pane.session_id = Some(session_id);
            pane.previous_ids.clear();
            Ok(vec![Effect::Persist])
        }
        Command::SessionRekeyed {
            session_id,
            previous_ids,
        } => Ok(rekey(model, session_id, previous_ids)),
        Command::PaneExited { .. } => Ok(vec![]),
    }
}

struct NewPane {
    kind: PaneKind,
    resume: Option<String>,
    prompt: Option<String>,
    account_id: Option<String>,
    label: Option<String>,
    command: Option<String>,
    codex_mode: Option<CodexMode>,
}

fn open_workspace(
    model: &mut Model,
    project_path: String,
    project_name: String,
    worktree_path: Option<String>,
    branch: Option<String>,
    renderer: Renderer,
    transient: bool,
) -> Vec<Effect> {
    let worktree_path = worktree_path.filter(|w| *w != project_path);
    if let Some(ws) = model
        .workspaces
        .iter_mut()
        .find(|w| w.project_path == project_path && w.worktree_path == worktree_path)
    {
        let opened = Effect::Opened {
            workspace_id: ws.id.clone(),
            tab_id: None,
            pane_id: None,
        };
        // Opened on purpose now: it stays when its last tab closes.
        if ws.transient && !transient {
            ws.transient = false;
            return vec![opened, Effect::Persist];
        }
        return vec![opened];
    }
    let ws = Workspace {
        id: new_id(),
        project_path,
        project_name,
        branch: worktree_path.as_ref().and(branch),
        worktree_path,
        renderer,
        tabs: Vec::new(),
        split_view: None,
        transient,
    };
    let opened = Effect::Opened {
        workspace_id: ws.id.clone(),
        tab_id: None,
        pane_id: None,
    };
    model.workspaces.push(ws);
    vec![opened, Effect::Persist]
}

fn new_session(model: &mut Model, target: Target, new: NewPane) -> Result<Vec<Effect>> {
    if new.kind == PaneKind::Shell && (new.resume.is_some() || new.prompt.is_some()) {
        bail!("A shell has no session to resume or prompt");
    }
    if new.kind.is_ai() && new.command.is_some() {
        bail!("Claude and Codex panes run their session, not a command");
    }
    if new.kind != PaneKind::Codex && new.codex_mode.is_some() {
        bail!("Only a Codex pane has a Codex mode");
    }
    if let Some(id) = new.resume.as_deref() {
        if let Some(pane) = model.pane_for_session(new.kind, id) {
            let (w, t, _) = model.pane_at(&pane.id).context("pane vanished")?;
            return Ok(vec![Effect::Opened {
                workspace_id: model.workspaces[w].id.clone(),
                tab_id: Some(model.workspaces[w].tabs[t].id.clone()),
                pane_id: Some(pane.id.clone()),
            }]);
        }
    }

    let w = match target {
        Target::Workspace { workspace_id } => workspace_index(model, &workspace_id)?,
        Target::Location {
            project_path,
            worktree_path,
            project_name,
            branch,
        } => {
            let name = project_name.unwrap_or_else(|| base_name(&project_path));
            let opened = open_workspace(
                model,
                project_path,
                name,
                worktree_path,
                branch,
                Renderer::Xterm,
                true,
            );
            let Some(Effect::Opened { workspace_id, .. }) = opened.first() else {
                unreachable!("open_workspace reports the workspace first");
            };
            workspace_index(model, workspace_id)?
        }
    };

    let mut effects = Vec::new();
    let ws = &mut model.workspaces[w];
    let label = new
        .label
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_label(ws, new.kind));
    let prompt = new
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    let pane = Pane {
        id: new_id(),
        kind: new.kind,
        // A Claude session gets its id up front, so every client knows it before Claude writes anything.
        session_id: match new.kind {
            PaneKind::Claude => Some(new.resume.clone().unwrap_or_else(new_id)),
            PaneKind::Codex => new.resume.clone(),
            PaneKind::Shell => None,
        },
        previous_ids: Vec::new(),
        account_id: new.account_id,
        // A resumed session already started: its prompt would be sent again.
        prompt: prompt.filter(|_| new.resume.is_none()),
        command: new.command,
        codex_mode: (new.kind == PaneKind::Codex).then(|| new.codex_mode.unwrap_or_default()),
    };
    let tab = Tab {
        id: new_id(),
        label,
        kind: new.kind,
        split: SplitDirection::Horizontal,
        panes: vec![pane],
    };
    effects.push(spawn(ws, &tab.panes[0], new.resume.is_some()));
    effects.push(Effect::Opened {
        workspace_id: ws.id.clone(),
        tab_id: Some(tab.id.clone()),
        pane_id: Some(tab.panes[0].id.clone()),
    });
    ws.tabs.push(tab);
    effects.push(Effect::Persist);
    Ok(effects)
}

fn base_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

fn default_label(ws: &Workspace, kind: PaneKind) -> String {
    match kind {
        // Shell tabs count every tab, as the desktop always has.
        PaneKind::Shell => format!("Terminal {}", ws.tabs.len() + 1),
        PaneKind::Claude | PaneKind::Codex => {
            let n = ws.tabs.iter().filter(|t| t.kind == kind).count() + 1;
            let prefix = if kind == PaneKind::Claude {
                "Claude"
            } else {
                "Codex"
            };
            format!("{prefix} {n}")
        }
    }
}

/// The effect that starts a pane's process.
fn spawn(ws: &Workspace, pane: &Pane, resume: bool) -> Effect {
    let pane_id = pane.id.clone();
    let cwd = ws.cwd().to_string();
    match pane.kind {
        PaneKind::Shell => Effect::SpawnShell {
            pane_id,
            cwd,
            renderer: ws.renderer,
            command: pane.command.clone(),
            account_id: pane.account_id.clone(),
        },
        PaneKind::Claude => Effect::SpawnClaude {
            pane_id,
            cwd,
            renderer: ws.renderer,
            session_id: pane.session_id.clone().unwrap_or_default(),
            resume,
            account_id: pane.account_id.clone(),
            prompt: pane.prompt.clone(),
        },
        PaneKind::Codex => Effect::SpawnCodex {
            pane_id,
            cwd,
            renderer: ws.renderer,
            session_id: pane.session_id.clone(),
            mode: pane.codex_mode.unwrap_or_default(),
            prompt: pane.prompt.clone().filter(|_| pane.session_id.is_none()),
        },
    }
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

fn close_tab(model: &mut Model, tab_id: &str) -> Result<Vec<Effect>> {
    let (w, t) = tab_index(model, tab_id)?;
    let ws = &mut model.workspaces[w];
    let tab = ws.tabs.remove(t);
    drop_split_with(ws, tab_id);
    if ws.transient && ws.tabs.is_empty() {
        model.workspaces.remove(w);
    }
    Ok(ended(&tab.panes))
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

/// Stops each pane and starts it again on the same session and account (the
/// transcript lives under that account). Pane and tab ids stay.
fn restart(model: &mut Model, tab_id: &str) -> Result<Vec<Effect>> {
    let (w, t) = tab_index(model, tab_id)?;
    let ws = &mut model.workspaces[w];
    if !ws.tabs[t].kind.is_ai() {
        bail!("Only a Claude or Codex tab restarts");
    }
    let mut effects = Vec::new();
    let mut minted = Vec::new();
    for pane in &mut ws.tabs[t].panes {
        effects.push(Effect::Stop {
            pane_id: pane.id.clone(),
            session_id: pane.session_id.clone(),
        });
        if pane.kind == PaneKind::Claude && pane.session_id.is_none() {
            pane.session_id = Some(new_id());
            minted.push(pane.id.clone());
        }
    }
    let ws = &model.workspaces[w];
    for pane in &ws.tabs[t].panes {
        let resume = pane.session_id.is_some() && !minted.contains(&pane.id);
        effects.push(spawn(ws, pane, resume));
    }
    effects.push(Effect::Opened {
        workspace_id: ws.id.clone(),
        tab_id: Some(tab_id.to_string()),
        pane_id: None,
    });
    if !minted.is_empty() {
        effects.push(Effect::Persist);
    }
    Ok(effects)
}

/// A Codex pane hands its thread between the TUI and `codex app-server`: the
/// old process stops before the new one starts, so one process owns the thread.
fn set_codex_mode(model: &mut Model, pane_id: &str, mode: CodexMode) -> Result<Vec<Effect>> {
    let (w, t, p) = pane_index(model, pane_id)?;
    let pane = &mut model.workspaces[w].tabs[t].panes[p];
    if pane.kind != PaneKind::Codex {
        bail!("Only a Codex pane switches between terminal and chat");
    }
    if pane.codex_mode == Some(mode) {
        return Ok(vec![]);
    }
    pane.codex_mode = Some(mode);
    let stop = Effect::Stop {
        pane_id: pane_id.to_string(),
        session_id: pane.session_id.clone(),
    };
    let ws = &model.workspaces[w];
    let pane = &ws.tabs[t].panes[p];
    Ok(vec![
        stop,
        spawn(ws, pane, pane.session_id.is_some()),
        Effect::Persist,
    ])
}

fn split(model: &mut Model, tab_id: &str, direction: SplitDirection) -> Result<Vec<Effect>> {
    let (w, t) = tab_index(model, tab_id)?;
    let ws = &mut model.workspaces[w];
    if ws.renderer == Renderer::Native {
        bail!("Native terminals can't be split");
    }
    let current = ws.split_view.as_ref().filter(|s| {
        s.tab_ids.iter().any(|id| id == tab_id)
            && s.tab_ids
                .iter()
                .all(|id| ws.tabs.iter().any(|t| t.id == *id))
    });
    if let Some(split) = current {
        ws.split_view = (split.direction != direction).then(|| SplitView {
            direction,
            tab_ids: split.tab_ids.clone(),
        });
        return Ok(vec![Effect::Persist]);
    }
    let mut effects = Vec::new();
    let partner = match ws
        .tabs
        .get(t + 1)
        .or_else(|| t.checked_sub(1).map(|i| &ws.tabs[i]))
    {
        Some(tab) => tab.id.clone(),
        None => {
            let pane = Pane {
                id: new_id(),
                kind: PaneKind::Shell,
                session_id: None,
                previous_ids: Vec::new(),
                account_id: None,
                prompt: None,
                command: None,
                codex_mode: None,
            };
            effects.push(spawn(ws, &pane, false));
            let tab = Tab {
                id: new_id(),
                label: default_label(ws, PaneKind::Shell),
                kind: PaneKind::Shell,
                split: SplitDirection::Horizontal,
                panes: vec![pane],
            };
            let id = tab.id.clone();
            ws.tabs.push(tab);
            id
        }
    };
    ws.split_view = Some(SplitView {
        direction,
        tab_ids: [tab_id.to_string(), partner],
    });
    effects.push(Effect::Persist);
    Ok(effects)
}

fn move_pane(model: &mut Model, pane_id: &str, tab_id: &str) -> Result<Vec<Effect>> {
    let (w, from, p) = pane_index(model, pane_id)?;
    let (to_w, to) = tab_index(model, tab_id)?;
    if w != to_w {
        bail!("A pane only moves within its workspace");
    }
    if from == to {
        return Ok(vec![]);
    }
    let ws = &mut model.workspaces[w];
    if ws.tabs[from].kind != ws.tabs[to].kind {
        bail!("A pane only moves into a tab of its own kind");
    }
    let pane = ws.tabs[from].panes.remove(p);
    ws.tabs[to].panes.push(pane);
    if ws.tabs[from].panes.is_empty() {
        let emptied = ws.tabs.remove(from).id;
        drop_split_with(ws, &emptied);
    }
    Ok(vec![Effect::Persist])
}

fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) -> Vec<Effect> {
    if from == to {
        return vec![];
    }
    let item = items.remove(from);
    items.insert(to, item);
    vec![Effect::Persist]
}

/// The pane on any of `previous_ids` follows its session to the new id.
fn rekey(model: &mut Model, session_id: String, previous_ids: Vec<String>) -> Vec<Effect> {
    let pane = model
        .workspaces
        .iter_mut()
        .flat_map(|w| &mut w.tabs)
        .flat_map(|t| &mut t.panes)
        .find(|p| {
            p.session_id
                .as_ref()
                .is_some_and(|id| *id == session_id || previous_ids.contains(id))
        });
    let Some(pane) = pane else {
        return vec![];
    };
    let mut ids = pane.previous_ids.clone();
    ids.extend(pane.session_id.clone());
    ids.extend(previous_ids);
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| *id != session_id && seen.insert(id.clone()));
    if ids == pane.previous_ids && pane.session_id.as_ref() == Some(&session_id) {
        return vec![];
    }
    pane.previous_ids = ids;
    pane.session_id = Some(session_id);
    vec![Effect::Persist]
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
