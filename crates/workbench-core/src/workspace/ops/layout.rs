//! Tab labels, splits and ordering.

use anyhow::{bail, Result};

use super::lifecycle::default_label;
use super::{drop_split_with, new_id, pane_index, spawn, tab_index, workspace_index};
use crate::workspace::command::Effect;
use crate::workspace::model::{Model, Pane, PaneKind, Renderer, SplitDirection, SplitView, Tab};

pub(super) fn rename(model: &mut Model, tab_id: &str, label: &str) -> Result<Vec<Effect>> {
    let label = label.trim();
    if label.is_empty() {
        bail!("A tab label can't be empty");
    }
    let (w, t) = tab_index(model, tab_id)?;
    let tab = &mut model.workspaces[w].tabs[t];
    if tab.label == label {
        return Ok(vec![]);
    }
    tab.label = label.to_string();
    Ok(vec![Effect::Persist])
}

pub(super) fn split(
    model: &mut Model,
    tab_id: &str,
    direction: SplitDirection,
) -> Result<Vec<Effect>> {
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
    let neighbour = ws
        .tabs
        .get(t + 1)
        .or_else(|| t.checked_sub(1).map(|i| &ws.tabs[i]));
    let partner = match neighbour {
        Some(tab) => tab.id.clone(),
        None => {
            let mut pane = Pane {
                id: new_id(),
                kind: PaneKind::Shell,
                session_id: None,
                previous_ids: Vec::new(),
                account_id: None,
                prompt: None,
                command: None,
                codex_mode: None,
            };
            effects.push(spawn(ws.cwd(), ws.renderer, &mut pane, false).0);
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

/// Every pane in a tab has the tab's kind, so a pane only joins a tab of its own.
pub(super) fn move_pane(model: &mut Model, pane_id: &str, tab_id: &str) -> Result<Vec<Effect>> {
    let (w, from, p) = pane_index(model, pane_id)?;
    let (to_w, to) = tab_index(model, tab_id)?;
    if w != to_w {
        bail!("A pane only moves within its workspace");
    }
    if from == to {
        return Ok(vec![]);
    }
    let ws = &mut model.workspaces[w];
    if ws.tabs[from].panes[p].kind != ws.tabs[to].kind {
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

pub(super) fn move_tab(model: &mut Model, tab_id: &str, to_tab_id: &str) -> Result<Vec<Effect>> {
    let (w, from) = tab_index(model, tab_id)?;
    let (to_w, to) = tab_index(model, to_tab_id)?;
    if w != to_w {
        bail!("Tabs only move within their workspace");
    }
    Ok(move_item(&mut model.workspaces[w].tabs, from, to))
}

pub(super) fn move_workspace(
    model: &mut Model,
    workspace_id: &str,
    to_workspace_id: &str,
) -> Result<Vec<Effect>> {
    let from = workspace_index(model, workspace_id)?;
    let to = workspace_index(model, to_workspace_id)?;
    Ok(move_item(&mut model.workspaces, from, to))
}

fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) -> Vec<Effect> {
    if from == to {
        return vec![];
    }
    let item = items.remove(from);
    items.insert(to, item);
    vec![Effect::Persist]
}
