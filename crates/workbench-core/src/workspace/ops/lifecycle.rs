//! Workspaces opening and closing, and sessions starting, restarting and
//! following their process.

use anyhow::{bail, Result};
use std::path::Path;

use super::{
    drop_split_with, ended, new_id, pane_index, spawn, spawn_at, tab_index, workspace_index,
};
use crate::workspace::command::{Effect, Target};
use crate::workspace::model::{
    same_path, CodexMode, Model, Pane, PaneKind, Renderer, SplitDirection, Tab, Workspace,
};

pub(super) struct NewPane {
    pub kind: PaneKind,
    pub resume: Option<String>,
    pub prompt: Option<String>,
    pub account_id: Option<String>,
    pub label: Option<String>,
    pub command: Option<String>,
    pub codex_mode: Option<CodexMode>,
}

/// The workspace's index, and the effects of opening it.
pub(super) fn open_workspace(
    model: &mut Model,
    project_path: String,
    project_name: String,
    worktree_path: Option<String>,
    branch: Option<String>,
    renderer: Renderer,
    transient: bool,
) -> (usize, Vec<Effect>) {
    let worktree_path = worktree_path.filter(|w| !same_path(w, &project_path));
    let existing = model.workspaces.iter().position(|w| {
        same_path(&w.project_path, &project_path)
            && match (&w.worktree_path, &worktree_path) {
                (None, None) => true,
                (Some(a), Some(b)) => same_path(a, b),
                _ => false,
            }
    });
    if let Some(i) = existing {
        let ws = &mut model.workspaces[i];
        let opened = Effect::Opened {
            workspace_id: ws.id.clone(),
            tab_id: None,
            pane_id: None,
        };
        // Opened on purpose now: it stays when its last tab closes.
        if ws.transient && !transient {
            ws.transient = false;
            return (i, vec![opened, Effect::Persist]);
        }
        return (i, vec![opened]);
    }
    let ws = Workspace {
        id: new_id(),
        project_path,
        project_name,
        // A main checkout's branch is read from git when shown.
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
    (model.workspaces.len() - 1, vec![opened, Effect::Persist])
}

pub(super) fn close_project(model: &mut Model, project_path: &str) -> Vec<Effect> {
    let (closing, kept) = std::mem::take(&mut model.workspaces)
        .into_iter()
        .partition::<Vec<_>, _>(|w| same_path(&w.project_path, project_path));
    model.workspaces = kept;
    if closing.is_empty() {
        return vec![];
    }
    ended(closing.iter().flat_map(|w| &w.tabs).flat_map(|t| &t.panes))
}

pub(super) fn new_session(model: &mut Model, target: Target, new: NewPane) -> Result<Vec<Effect>> {
    if new.kind == PaneKind::Shell && (new.resume.is_some() || new.prompt.is_some()) {
        bail!("A shell has no session to resume or prompt");
    }
    if new.kind.is_ai() && new.command.is_some() {
        bail!("Claude and Codex panes run their session, not a command");
    }
    if new.kind != PaneKind::Codex && new.codex_mode.is_some() {
        bail!("Only a Codex pane has a Codex mode");
    }
    if let Some((w, t, p)) = new
        .resume
        .as_deref()
        .and_then(|id| model.session_at(new.kind, id))
    {
        let ws = &model.workspaces[w];
        return Ok(vec![Effect::Opened {
            workspace_id: ws.id.clone(),
            tab_id: Some(ws.tabs[t].id.clone()),
            pane_id: Some(ws.tabs[t].panes[p].id.clone()),
        }]);
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
            let (w, _) = open_workspace(
                model,
                project_path,
                name,
                worktree_path,
                branch,
                Renderer::Xterm,
                true,
            );
            w
        }
    };

    let ws = &mut model.workspaces[w];
    let label = new
        .label
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_label(ws, new.kind));
    let mut pane = Pane {
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
        prompt: new
            .prompt
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty() && new.resume.is_none())
            .map(str::to_string),
        command: new.command,
        codex_mode: (new.kind == PaneKind::Codex).then(|| new.codex_mode.unwrap_or_default()),
    };
    let (spawned, _) = spawn(ws.cwd(), ws.renderer, &mut pane, new.resume.is_some());
    let tab_id = new_id();
    let opened = Effect::Opened {
        workspace_id: ws.id.clone(),
        tab_id: Some(tab_id.clone()),
        pane_id: Some(pane.id.clone()),
    };
    ws.tabs.push(Tab {
        id: tab_id,
        label,
        kind: new.kind,
        split: SplitDirection::Horizontal,
        panes: vec![pane],
    });
    Ok(vec![spawned, opened, Effect::Persist])
}

fn base_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

pub(super) fn default_label(ws: &Workspace, kind: PaneKind) -> String {
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

pub(super) fn close_pane(model: &mut Model, pane_id: &str) -> Result<Vec<Effect>> {
    let (w, t, p) = pane_index(model, pane_id)?;
    let tab = &mut model.workspaces[w].tabs[t];
    if tab.panes.len() > 1 {
        let pane = tab.panes.remove(p);
        return Ok(ended([&pane]));
    }
    let tab_id = tab.id.clone();
    close_tab(model, &tab_id)
}

pub(super) fn close_tab(model: &mut Model, tab_id: &str) -> Result<Vec<Effect>> {
    let (w, t) = tab_index(model, tab_id)?;
    let ws = &mut model.workspaces[w];
    let tab = ws.tabs.remove(t);
    drop_split_with(ws, tab_id);
    if ws.transient && ws.tabs.is_empty() {
        model.workspaces.remove(w);
    }
    Ok(ended(&tab.panes))
}

/// Stops each pane and starts it again on the same session and account (the
/// transcript lives under that account). Pane and tab ids stay; a prompt
/// already sent is never sent again.
pub(super) fn restart(model: &mut Model, tab_id: &str) -> Result<Vec<Effect>> {
    let (w, t) = tab_index(model, tab_id)?;
    let tab = &model.workspaces[w].tabs[t];
    if tab.panes.iter().any(|p| !p.kind.is_ai()) {
        bail!("Only a Claude or Codex tab restarts");
    }
    let mut effects: Vec<Effect> = tab
        .panes
        .iter()
        .map(|p| Effect::Stop {
            pane_id: p.id.clone(),
            session_id: p.session_id.clone(),
        })
        .collect();
    let mut changed = false;
    for p in 0..tab.panes.len() {
        let resume = model.workspaces[w].tabs[t].panes[p].session_id.is_some();
        let (spawned, consumed) = spawn_at(model, (w, t, p), resume);
        effects.push(spawned);
        changed |= consumed;
    }
    effects.push(Effect::Opened {
        workspace_id: model.workspaces[w].id.clone(),
        tab_id: Some(tab_id.to_string()),
        pane_id: None,
    });
    if changed {
        effects.push(Effect::Persist);
    }
    Ok(effects)
}

/// A Codex pane hands its thread between the TUI and `codex app-server`: the
/// old process stops before the new one starts, so one process owns the thread.
pub(super) fn set_codex_mode(
    model: &mut Model,
    pane_id: &str,
    mode: CodexMode,
) -> Result<Vec<Effect>> {
    let at @ (w, t, p) = pane_index(model, pane_id)?;
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
    let resume = pane.session_id.is_some();
    let (spawned, _) = spawn_at(model, at, resume);
    Ok(vec![stop, spawned, Effect::Persist])
}

/// The pane's process now runs `session_id` (a Codex thread got its id,
/// `/resume` switched conversation). One pane per session: when another pane
/// holds it, this is refused and the caller stops the reporting pane's process.
pub(super) fn attach(model: &mut Model, pane_id: &str, session_id: String) -> Result<Vec<Effect>> {
    // Folded-in events can race a close: an unknown pane is a no-op.
    let Some((w, t, p)) = model.pane_at(pane_id) else {
        return Ok(vec![]);
    };
    let kind = model.workspaces[w].tabs[t].panes[p].kind;
    if let Some((ow, ot, op)) = model.session_at(kind, &session_id) {
        let holder = &model.workspaces[ow].tabs[ot].panes[op].id;
        if holder != pane_id {
            bail!("Session {session_id} is already open in pane {holder}");
        }
    }
    let pane = &mut model.workspaces[w].tabs[t].panes[p];
    if pane.session_id.as_deref() == Some(session_id.as_str()) {
        return Ok(vec![]);
    }
    // Another conversation: the old one's `/clear` ids no longer apply.
    pane.session_id = Some(session_id);
    pane.previous_ids.clear();
    Ok(vec![Effect::Persist])
}

/// The pane on any of `previous_ids` follows its session to the new id.
pub(super) fn rekey(
    model: &mut Model,
    session_id: String,
    previous_ids: Vec<String>,
) -> Vec<Effect> {
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
