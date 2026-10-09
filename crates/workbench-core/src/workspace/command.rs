//! The commands clients and the server send to change the model, and the
//! effects `ops::apply` returns for the caller to carry out.

use serde::{Deserialize, Serialize};

use super::model::{CodexMode, PaneKind, Renderer, SplitDirection};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum Target {
    Workspace {
        workspace_id: String,
    },
    Location {
        project_path: String,
        #[serde(default)]
        worktree_path: Option<String>,
        /// Used only when the workspace has to be opened; defaults to the folder name.
        #[serde(default)]
        project_name: Option<String>,
        #[serde(default)]
        branch: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    OpenWorkspace {
        project_path: String,
        project_name: String,
        #[serde(default)]
        worktree_path: Option<String>,
        #[serde(default)]
        branch: Option<String>,
        #[serde(default)]
        renderer: Renderer,
    },
    CloseWorkspace {
        workspace_id: String,
    },
    CloseProject {
        project_path: String,
    },
    NewSession {
        #[serde(flatten)]
        target: Target,
        kind: PaneKind,
        /// Continue this session; a pane already running it is returned instead.
        #[serde(default)]
        resume: Option<String>,
        #[serde(default)]
        prompt: Option<String>,
        /// Resolved by the caller (project default, else the active account).
        #[serde(default)]
        account_id: Option<String>,
        /// Defaults to `Claude N` / `Codex N` / `Terminal N`.
        #[serde(default)]
        label: Option<String>,
        /// Shell only: typed into the shell (a task).
        #[serde(default)]
        command: Option<String>,
        /// Codex only; defaults to the TUI.
        #[serde(default)]
        codex_mode: Option<CodexMode>,
    },
    /// Ends the pane's process everywhere; the tab goes with its last pane.
    ClosePane {
        pane_id: String,
    },
    CloseTab {
        tab_id: String,
    },
    Restart {
        tab_id: String,
    },
    SetCodexMode {
        pane_id: String,
        mode: CodexMode,
    },
    Rename {
        tab_id: String,
        label: String,
    },
    /// Shows the tab beside its neighbour (a new shell tab when it is alone);
    /// the same direction again unsplits.
    Split {
        tab_id: String,
        direction: SplitDirection,
    },
    /// Moves a pane into another tab of its workspace.
    MovePane {
        pane_id: String,
        tab_id: String,
    },
    /// Moves a tab to another tab's position in the strip.
    MoveTab {
        tab_id: String,
        to_tab_id: String,
    },
    /// Moves a workspace to another workspace's position.
    MoveWorkspace {
        workspace_id: String,
        to_workspace_id: String,
    },
    TrustFolder {
        pane_id: String,
    },
    /// A project moved or was renamed: its workspaces follow. Worktree paths stay.
    UpdateProject {
        project_path: String,
        new_path: String,
        project_name: String,
    },
    /// Folded in by the server: the pane's session moved to another Claude
    /// account (`""`: the default login), so later spawns use it.
    AccountMoved {
        pane_id: String,
        account_id: String,
    },
    /// Folded in by the server: the pane's process runs this session (a Codex
    /// thread got its id, `/resume` switched conversation).
    SessionAttached {
        pane_id: String,
        session_id: String,
    },
    /// Folded in by the server: `/clear` gave a session a new id.
    SessionRekeyed {
        session_id: String,
        previous_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Effect {
    SpawnShell {
        pane_id: String,
        cwd: String,
        renderer: Renderer,
        command: Option<String>,
        account_id: Option<String>,
    },
    /// `resume`: the session may already exist (the launcher still checks for its history).
    SpawnClaude {
        pane_id: String,
        cwd: String,
        renderer: Renderer,
        session_id: String,
        resume: bool,
        account_id: Option<String>,
        prompt: Option<String>,
    },
    /// A `session_id` with no thread on disk yet starts a fresh one.
    SpawnCodex {
        pane_id: String,
        cwd: String,
        renderer: Renderer,
        session_id: Option<String>,
        mode: CodexMode,
        prompt: Option<String>,
    },
    /// Stop the pane's process; the pane stays (restart, mode switch).
    Stop {
        pane_id: String,
        session_id: Option<String>,
    },
    /// The pane is gone: end its process and session for every device.
    End {
        pane_id: String,
        session_id: Option<String>,
    },
    /// Answer the Claude folder trust dialog for the pane.
    TrustFolder { pane_id: String, cwd: String },
    /// What the command opened or found; the issuing client selects it.
    Opened {
        workspace_id: String,
        tab_id: Option<String>,
        pane_id: Option<String>,
    },
    /// The model changed: save it.
    Persist,
}
