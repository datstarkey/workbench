//! The model plus the desktop's local state, versioned, in `workspaces.v2.json`.
//! The old desktop snapshot (`workspaces.json`, no `version`) is migrated on
//! load when no v2 file exists, and never written: an older build still reads it.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use super::model::{
    same_path, CodexMode, Model, Pane, PaneKind, Renderer, SplitDirection, SplitView, Tab,
    Workspace,
};
use super::ops::new_id;
use crate::paths;

pub const FILE: &str = "workspaces.v2.json";
pub const LEGACY_FILE: &str = "workspaces.json";
pub const VERSION: u32 = 2;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacesFile {
    #[serde(flatten)]
    pub model: Model,
    #[serde(default)]
    pub local: LocalState,
}

/// What the desktop keeps per device; not part of the shared model. It lives
/// in this file until the desktop moves it to its own storage.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_id: Option<String>,
    /// workspace id → its active tab id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub active_tab_ids: BTreeMap<String, String>,
    /// Claude panes shown as chat (a display choice; Codex's is `codexMode`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chat_panes: Vec<String>,
    /// pane id → the server terminal it re-attaches to after a webview reload.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub server_terminal_ids: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct Versioned<'a> {
    version: u32,
    #[serde(flatten)]
    file: &'a WorkspacesFile,
}

/// `dir`'s `workspaces.v2.json`, else its migrated `workspaces.json`, else empty.
pub fn load(dir: &Path) -> Result<WorkspacesFile> {
    for name in [FILE, LEGACY_FILE] {
        let path = dir.join(name);
        if path.exists() {
            return parse(&std::fs::read_to_string(path)?);
        }
    }
    Ok(WorkspacesFile::default())
}

/// Writes `dir`'s `workspaces.v2.json`; `workspaces.json` is left as it was.
pub fn save(dir: &Path, file: &WorkspacesFile) -> Result<()> {
    paths::save_json(
        &dir.join(FILE),
        &Versioned {
            version: VERSION,
            file,
        },
    )
}

/// No `version`, or `1`, is the old desktop snapshot. Anything but that or the
/// current version is refused rather than guessed at.
pub fn parse(content: &str) -> Result<WorkspacesFile> {
    let value: serde_json::Value = serde_json::from_str(content)?;
    let version = match value.get("version") {
        None => 1,
        Some(v) => match v.as_u64() {
            Some(n @ (1 | 2)) => n,
            _ => bail!(
                "Workspaces file version {v} is not one this Workbench reads (1 to {VERSION})"
            ),
        },
    };
    if version == 1 {
        Ok(migrate_v1(serde_json::from_value(value)?))
    } else {
        Ok(serde_json::from_value(value)?)
    }
}

// The desktop snapshot before the model (`types::WorkspaceFile`, plus what the
// webview sends that older Rust dropped: `renderer`, per-pane `serverTerminalId`).

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1File {
    #[serde(default)]
    workspaces: Vec<V1Workspace>,
    #[serde(default)]
    selected_id: Option<String>,
    #[serde(default)]
    server_terminal_ids: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1Workspace {
    id: String,
    project_path: String,
    project_name: String,
    #[serde(default)]
    terminal_tabs: Vec<V1Tab>,
    #[serde(default)]
    active_terminal_tab_id: String,
    #[serde(default)]
    split_view: Option<V1SplitView>,
    #[serde(default)]
    worktree_path: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    renderer: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1SplitView {
    direction: String,
    tab_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1Tab {
    id: String,
    label: String,
    #[serde(default)]
    split: Option<String>,
    #[serde(default)]
    panes: Vec<V1Pane>,
    #[serde(rename = "type", default)]
    session_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1Pane {
    id: String,
    #[serde(default)]
    startup_command: Option<String>,
    #[serde(rename = "type", default)]
    session_type: Option<String>,
    #[serde(default)]
    claude_session_id: Option<String>,
    #[serde(default)]
    view: Option<String>,
    #[serde(default)]
    claude_account_id: Option<String>,
    #[serde(default)]
    claude_prompt: Option<String>,
    #[serde(default)]
    server_terminal_id: Option<String>,
}

fn kind(session_type: Option<&str>) -> PaneKind {
    match session_type {
        Some("claude") => PaneKind::Claude,
        Some("codex") => PaneKind::Codex,
        _ => PaneKind::Shell,
    }
}

fn direction(s: &str) -> Option<SplitDirection> {
    match s {
        "horizontal" => Some(SplitDirection::Horizontal),
        "vertical" => Some(SplitDirection::Vertical),
        _ => None,
    }
}

fn migrate_v1(old: V1File) -> WorkspacesFile {
    let mut local = LocalState {
        selected_id: old.selected_id,
        server_terminal_ids: old.server_terminal_ids,
        ..LocalState::default()
    };
    let workspaces = old
        .workspaces
        .into_iter()
        .map(|w| migrate_workspace(w, &mut local))
        .collect();
    WorkspacesFile {
        model: Model { workspaces },
        local,
    }
}

fn migrate_workspace(w: V1Workspace, local: &mut LocalState) -> Workspace {
    let tabs: Vec<Tab> = w
        .terminal_tabs
        .into_iter()
        .flat_map(|t| migrate_tab(t, local))
        .collect();
    if tabs.iter().any(|t| t.id == w.active_terminal_tab_id) {
        local
            .active_tab_ids
            .insert(w.id.clone(), w.active_terminal_tab_id);
    }
    let split_view = w.split_view.and_then(|s| {
        let [a, b]: [String; 2] = s.tab_ids.try_into().ok()?;
        let exists = |id: &String| tabs.iter().any(|t| t.id == *id);
        (exists(&a) && exists(&b)).then_some(SplitView {
            direction: direction(&s.direction)?,
            tab_ids: [a, b],
        })
    });
    let worktree_path = w.worktree_path.filter(|p| !same_path(p, &w.project_path));
    Workspace {
        id: w.id,
        project_path: w.project_path,
        project_name: w.project_name,
        // A main checkout's branch is read from git when shown.
        branch: worktree_path.as_ref().and(w.branch),
        worktree_path,
        renderer: match w.renderer.as_deref() {
            Some("native") => Renderer::Native,
            _ => Renderer::Xterm,
        },
        tabs,
        split_view,
        transient: false,
    }
}

/// Every pane in a tab has the tab's kind: a tab mixing kinds is split into
/// one tab per kind (in pane order, the first keeping its id), and the tab
/// takes its panes' kind over its own `type`.
fn migrate_tab(t: V1Tab, local: &mut LocalState) -> Vec<Tab> {
    let tab_kind = kind(t.session_type.as_deref());
    let mut panes: Vec<Pane> = t
        .panes
        .into_iter()
        .map(|p| migrate_pane(p, tab_kind, local))
        .collect();
    // An empty tab gets a shell, as the desktop's `ensureShape` does.
    if panes.is_empty() {
        panes.push(migrate_pane(
            V1Pane {
                id: new_id(),
                startup_command: None,
                session_type: Some("shell".into()),
                claude_session_id: None,
                view: None,
                claude_account_id: None,
                claude_prompt: None,
                server_terminal_id: None,
            },
            tab_kind,
            local,
        ));
    }
    let split = t.split.as_deref().and_then(direction).unwrap_or_default();
    let mut tabs: Vec<Tab> = Vec::new();
    for pane in panes {
        match tabs.iter_mut().find(|tab| tab.kind == pane.kind) {
            Some(tab) => tab.panes.push(pane),
            None => tabs.push(Tab {
                id: if tabs.is_empty() {
                    t.id.clone()
                } else {
                    new_id()
                },
                label: t.label.clone(),
                kind: pane.kind,
                split,
                panes: vec![pane],
            }),
        }
    }
    tabs
}

fn migrate_pane(p: V1Pane, tab_kind: PaneKind, local: &mut LocalState) -> Pane {
    let pane_kind = match p.session_type.as_deref() {
        Some(_) => kind(p.session_type.as_deref()),
        None => tab_kind,
    };
    if let Some(terminal) = p.server_terminal_id {
        local
            .server_terminal_ids
            .entry(p.id.clone())
            .or_insert(terminal);
    }
    let chat = p.view.as_deref() == Some("chat");
    let session_id = p.claude_session_id.filter(|s| !s.is_empty());
    let mut pane = Pane {
        id: p.id,
        kind: pane_kind,
        session_id: None,
        previous_ids: Vec::new(),
        account_id: p.claude_account_id,
        prompt: None,
        command: None,
        codex_mode: None,
    };
    match pane_kind {
        PaneKind::Shell => pane.command = p.startup_command,
        // A Claude pane runs its session, never a command; one saved without an
        // id (an older `claude …` command) starts a new session.
        PaneKind::Claude => {
            pane.session_id = Some(session_id.unwrap_or_else(new_id));
            pane.prompt = p.claude_prompt;
            if chat {
                local.chat_panes.push(pane.id.clone());
            }
        }
        // The command is rebuilt from launch options at spawn; only a new
        // session's prompt argument carries over.
        PaneKind::Codex => {
            if session_id.is_none() {
                pane.prompt = p.startup_command.as_deref().and_then(codex_prompt);
            }
            pane.session_id = session_id;
            pane.codex_mode = Some(if chat {
                CodexMode::AppServer
            } else {
                CodexMode::Tui
            });
        }
    }
    pane
}

const CODEX_OVERRIDE_KEYS: [&str; 3] = ["tui.alternate_screen", "approval_policy", "sandbox_mode"];

/// The prompt a `codex [--no-daemon] [-c k=v …] '<prompt>'` launch submits
/// (the desktop's `extractCodexPromptArg`, unquoted).
fn codex_prompt(command: &str) -> Option<String> {
    let mut rest = command.trim().strip_prefix("codex ")?.trim_start();
    rest = rest
        .strip_prefix("--no-daemon")
        .map_or(rest, str::trim_start);
    while let Some(after) = rest.strip_prefix("-c") {
        let after = after.trim_start_matches([' ', '\t']);
        if after.len() == rest.len() - 2
            || !CODEX_OVERRIDE_KEYS
                .iter()
                .any(|k| after.starts_with(&format!("{k}=")))
        {
            break;
        }
        let end = after.find(char::is_whitespace).unwrap_or(after.len());
        rest = after[end..].trim_start_matches([' ', '\t']);
    }
    let unquoted = if let Some(inner) = rest.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        inner.replace("'\"'\"'", "'")
    } else if let Some(inner) = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        inner.replace("\\\"", "\"")
    } else {
        rest.to_string()
    };
    (!unquoted.is_empty()).then_some(unquoted)
}

#[cfg(test)]
mod tests;
