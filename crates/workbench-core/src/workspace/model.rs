//! The shared workspace → tab → pane structure. Only what every device must
//! agree on lives here: which tab is selected, split ratios, a Claude pane's
//! Terminal/Chat display and process runtime (terminal id, status, title) are
//! per device or per process, never part of the model.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub workspaces: Vec<Workspace>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub project_path: String,
    pub project_name: String,
    /// Set for a worktree workspace; its panes run there instead of `project_path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default)]
    pub renderer: Renderer,
    pub tabs: Vec<Tab>,
    /// Two tabs shown side by side or stacked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_view: Option<SplitView>,
    /// Opened only to host a session started elsewhere: closes with its last tab.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub transient: bool,
}

impl Workspace {
    /// The directory its panes run in.
    pub fn cwd(&self) -> &str {
        self.worktree_path.as_deref().unwrap_or(&self.project_path)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub id: String,
    pub label: String,
    /// Every pane in the tab has this kind.
    pub kind: PaneKind,
    /// How the tab lays out more than one pane.
    #[serde(default)]
    pub split: SplitDirection,
    pub panes: Vec<Pane>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pane {
    pub id: String,
    pub kind: PaneKind,
    /// Claude: picked up front, so never absent. Codex: the thread, once it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Ids the session had before a `/clear`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_ids: Vec<String>,
    /// Saved Claude account the pane's process runs under; absent is the default login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// An agent action's prompt, submitted when the session is new.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Shell panes only: typed into the shell at start (a task, a legacy startup command).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Codex panes only: it picks the process, so it is shared state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_mode: Option<CodexMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaneKind {
    Shell,
    Claude,
    Codex,
}

impl PaneKind {
    pub fn is_ai(self) -> bool {
        self != PaneKind::Shell
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Renderer {
    #[default]
    Xterm,
    /// macOS SwiftTerm views; they can't be split.
    Native,
}

/// A Codex pane runs either its TUI in a terminal or `codex app-server` for chat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexMode {
    #[default]
    Tui,
    AppServer,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SplitDirection {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitView {
    pub direction: SplitDirection,
    pub tab_ids: [String; 2],
}

impl Model {
    pub fn workspace(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    /// `(workspace index, tab index)` of a tab.
    pub fn tab_at(&self, tab_id: &str) -> Option<(usize, usize)> {
        self.workspaces
            .iter()
            .enumerate()
            .find_map(|(w, ws)| ws.tabs.iter().position(|t| t.id == tab_id).map(|t| (w, t)))
    }

    /// `(workspace index, tab index, pane index)` of a pane.
    pub fn pane_at(&self, pane_id: &str) -> Option<(usize, usize, usize)> {
        self.workspaces.iter().enumerate().find_map(|(w, ws)| {
            ws.tabs.iter().enumerate().find_map(|(t, tab)| {
                tab.panes
                    .iter()
                    .position(|p| p.id == pane_id)
                    .map(|p| (w, t, p))
            })
        })
    }

    pub fn pane(&self, pane_id: &str) -> Option<&Pane> {
        self.pane_at(pane_id)
            .map(|(w, t, p)| &self.workspaces[w].tabs[t].panes[p])
    }

    /// The pane running a session, by its id or one it had before a `/clear`.
    pub fn pane_for_session(&self, kind: PaneKind, session_id: &str) -> Option<&Pane> {
        self.session_at(kind, session_id)
            .map(|(w, t, p)| &self.workspaces[w].tabs[t].panes[p])
    }

    /// `(workspace, tab, pane)` indices of `pane_for_session`.
    pub fn session_at(&self, kind: PaneKind, session_id: &str) -> Option<(usize, usize, usize)> {
        self.workspaces.iter().enumerate().find_map(|(w, ws)| {
            ws.tabs.iter().enumerate().find_map(|(t, tab)| {
                tab.panes
                    .iter()
                    .position(|p| {
                        p.kind == kind
                            && (p.session_id.as_deref() == Some(session_id)
                                || p.previous_ids.iter().any(|id| id == session_id))
                    })
                    .map(|p| (w, t, p))
            })
        })
    }
}

/// Whether two paths name the same folder: trailing separators don't count,
/// and on Windows neither do case or `\` vs `/`.
pub fn same_path(a: &str, b: &str) -> bool {
    fn normal(p: &str) -> String {
        let p = if cfg!(windows) {
            p.replace('\\', "/").to_lowercase()
        } else {
            p.to_string()
        };
        p.trim_end_matches('/').to_string()
    }
    normal(a) == normal(b)
}
