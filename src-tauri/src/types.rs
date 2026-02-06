use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectsFile {
    pub projects: Vec<ProjectConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTerminalRequest {
    pub id: String,
    pub project_path: String,
    pub shell: String,
    pub cols: u16,
    pub rows: u16,
    pub startup_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTerminalResponse {
    pub id: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalDataEvent {
    pub session_id: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalExitEvent {
    pub session_id: String,
    pub exit_code: i32,
    pub signal: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalWritePayload {
    pub session_id: String,
    pub data: String,
}

// Workspace persistence types

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalPaneSnapshot {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_command: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub session_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalTabSnapshot {
    pub id: String,
    pub label: String,
    pub split: String,
    pub panes: Vec<TerminalPaneSnapshot>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub session_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub id: String,
    pub project_path: String,
    pub project_name: String,
    pub terminal_tabs: Vec<TerminalTabSnapshot>,
    pub active_terminal_tab_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFile {
    pub workspaces: Vec<WorkspaceSnapshot>,
    pub selected_id: Option<String>,
}

// Claude CLI session discovery types

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredClaudeSession {
    pub session_id: String,
    pub label: String,
    pub timestamp: String,
}

// Claude Code settings types

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub name: String,
    pub description: String,
    pub version: String,
    pub dir_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub name: String,
    pub dir_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookScriptInfo {
    pub name: String,
    pub path: String,
}
