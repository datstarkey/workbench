use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTask {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<ProjectTask>,
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
pub struct TerminalActivityEvent {
    pub session_id: String,
    pub active: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message_role: Option<String>,
}

// Git types

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    pub branch: String,
    pub repo_root: String,
    pub is_worktree: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    pub path: String,
    pub head: String,
    pub branch: String,
    pub is_main: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    pub name: String,
    pub sha: String,
    pub is_current: bool,
    pub is_remote: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileStatus {
    pub path: String,
    pub status: String,
    pub staged: bool,
    pub unstaged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusResult {
    pub branch: String,
    pub files: Vec<GitFileStatus>,
    pub ahead: u32,
    pub behind: u32,
    pub has_upstream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLogEntry {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    pub author: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStashEntry {
    pub index: u32,
    pub message: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitResult {
    pub sha: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCopyOptions {
    pub ai_config: bool,
    pub env_files: bool,
}

impl Default for WorktreeCopyOptions {
    fn default() -> Self {
        Self {
            ai_config: true,
            env_files: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorktreeRequest {
    pub repo_path: String,
    pub branch: String,
    pub new_branch: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_options: Option<WorktreeCopyOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_point: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_before_create: Option<bool>,
}

// Workbench app settings

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAction {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default = "default_agent_action_target")]
    pub target: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_agent_action_target() -> String {
    "both".to_string()
}

fn default_agent_actions() -> Vec<AgentAction> {
    vec![
        AgentAction {
            id: "builtin-pr-review".to_string(),
            name: "PR Regression Review".to_string(),
            prompt: "Review this PR for regressions, weak abstractions, risky changes, and missing tests. Prioritize high-severity findings with file references.".to_string(),
            target: "both".to_string(),
            category: "Code Review".to_string(),
            tags: vec![
                "review".to_string(),
                "regressions".to_string(),
                "tests".to_string(),
            ],
        },
        AgentAction {
            id: "builtin-dry-optimization".to_string(),
            name: "DRY + Optimization Audit".to_string(),
            prompt: "Audit this codebase for DRY principle violations, duplication, and refactoring opportunities. Recommend concrete optimizations with tradeoffs.".to_string(),
            target: "both".to_string(),
            category: "Architecture".to_string(),
            tags: vec![
                "dry".to_string(),
                "refactor".to_string(),
                "performance".to_string(),
            ],
        },
        AgentAction {
            id: "builtin-security-scan".to_string(),
            name: "Security Scan".to_string(),
            prompt: "Review this codebase for security risks including injection paths, auth/authz mistakes, secret handling, dependency risk, and unsafe defaults. Provide prioritized mitigations.".to_string(),
            target: "both".to_string(),
            category: "Security".to_string(),
            tags: vec![
                "security".to_string(),
                "auth".to_string(),
                "dependencies".to_string(),
            ],
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchSettings {
    #[serde(default = "default_worktree_strategy")]
    pub worktree_strategy: String,
    #[serde(default)]
    pub trello_enabled: bool,
    #[serde(default = "default_terminal_performance_mode")]
    pub terminal_performance_mode: String,
    #[serde(default)]
    pub terminal_telemetry_enabled: bool,
    #[serde(default = "default_agent_actions")]
    pub agent_actions: Vec<AgentAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_hooks_approved: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_config_approved: Option<bool>,
    #[serde(default)]
    pub git_sidebar_enabled: bool,
    #[serde(default = "default_true")]
    pub worktree_fetch_before_create: bool,
    #[serde(default = "default_worktree_start_point")]
    pub worktree_start_point: String,
}

fn default_worktree_strategy() -> String {
    "sibling".to_string()
}

fn default_terminal_performance_mode() -> String {
    "auto".to_string()
}

fn default_true() -> bool {
    true
}

fn default_worktree_start_point() -> String {
    "auto".to_string()
}

impl Default for WorkbenchSettings {
    fn default() -> Self {
        Self {
            worktree_strategy: default_worktree_strategy(),
            trello_enabled: false,
            terminal_performance_mode: default_terminal_performance_mode(),
            terminal_telemetry_enabled: false,
            agent_actions: default_agent_actions(),
            claude_hooks_approved: None,
            codex_config_approved: None,
            git_sidebar_enabled: false,
            worktree_fetch_before_create: true,
            worktree_start_point: default_worktree_start_point(),
        }
    }
}

// Integration status for approval dialog

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationStatus {
    pub needs_changes: bool,
    pub description: String,
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

// Git filesystem watcher event

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitChangedEvent {
    pub project_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRefreshRequestedEvent {
    pub project_path: String,
    pub source: String,
    pub trigger: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubProjectStatusEvent {
    pub project_path: String,
    pub status: GitHubProjectStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubCheckTransitionEvent {
    pub project_path: String,
    pub pr_number: u64,
    pub name: String,
    pub bucket: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloMergeActionAppliedEvent {
    pub project_path: String,
    pub branch: String,
    pub card_id: String,
}

// GitHub integration types

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRemote {
    pub owner: String,
    pub repo: String,
    pub html_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubChecksStatus {
    pub overall: String,
    pub total: u32,
    pub passing: u32,
    pub failing: u32,
    pub pending: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPR {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub url: String,
    pub is_draft: bool,
    pub head_ref_name: String,
    pub review_decision: Option<String>,
    pub checks_status: GitHubChecksStatus,
    pub merge_state_status: Option<String>,
    pub actions: GitHubPRActions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPRActions {
    pub can_merge: bool,
    pub can_mark_ready: bool,
    pub can_update_branch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubProjectStatus {
    pub remote: Option<GitHubRemote>,
    pub prs: Vec<GitHubPR>,
    pub branch_runs: HashMap<String, GitHubBranchRuns>,
    pub pr_checks: HashMap<u64, Vec<GitHubCheckDetail>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubCheckDetail {
    pub name: String,
    pub bucket: String,
    pub workflow: String,
    pub link: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubWorkflowRun {
    #[serde(alias = "databaseId")]
    pub id: u64,
    pub name: String,
    pub display_title: String,
    pub head_branch: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub url: String,
    pub event: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubBranchRuns {
    pub status: GitHubChecksStatus,
    pub runs: Vec<GitHubWorkflowRun>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    // ProjectConfig round-trip

    #[test]
    fn project_config_round_trip() {
        let config = ProjectConfig {
            name: "my-project".to_string(),
            path: "/home/user/project".to_string(),
            shell: Some("/bin/zsh".to_string()),
            startup_command: Some("echo hello".to_string()),
            tasks: vec![ProjectTask {
                name: "build".to_string(),
                command: "cargo build".to_string(),
            }],
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ProjectConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "my-project");
        assert_eq!(deserialized.path, "/home/user/project");
        assert_eq!(deserialized.shell, Some("/bin/zsh".to_string()));
        assert_eq!(deserialized.startup_command, Some("echo hello".to_string()));
        assert_eq!(deserialized.tasks.len(), 1);
        assert_eq!(deserialized.tasks[0].name, "build");
    }

    #[test]
    fn project_config_camel_case_fields() {
        let config = ProjectConfig {
            name: "test".to_string(),
            path: "/tmp".to_string(),
            shell: Some("bash".to_string()),
            startup_command: Some("ls".to_string()),
            tasks: vec![],
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"startupCommand\""));
        assert!(!json.contains("\"startup_command\""));
    }

    #[test]
    fn project_config_optional_fields_none() {
        let config = ProjectConfig {
            name: "minimal".to_string(),
            path: "/tmp".to_string(),
            shell: None,
            startup_command: None,
            tasks: vec![],
        };
        let json = serde_json::to_string(&config).unwrap();
        // skip_serializing_if = "Option::is_none" should omit shell and startupCommand
        assert!(!json.contains("shell"));
        assert!(!json.contains("startupCommand"));
        // skip_serializing_if = "Vec::is_empty" should omit tasks
        assert!(!json.contains("tasks"));

        // Deserialize back with missing optional fields
        let json_str = r#"{"name":"minimal","path":"/tmp"}"#;
        let deserialized: ProjectConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(deserialized.shell, None);
        assert_eq!(deserialized.startup_command, None);
        assert!(deserialized.tasks.is_empty());
    }

    #[test]
    fn project_config_with_tasks() {
        let config = ProjectConfig {
            name: "tasked".to_string(),
            path: "/project".to_string(),
            shell: None,
            startup_command: None,
            tasks: vec![
                ProjectTask {
                    name: "test".to_string(),
                    command: "cargo test".to_string(),
                },
                ProjectTask {
                    name: "lint".to_string(),
                    command: "cargo clippy".to_string(),
                },
            ],
        };
        let json = serde_json::to_string(&config).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let tasks = val["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0]["name"], "test");
        assert_eq!(tasks[1]["command"], "cargo clippy");
    }

    // WorkspaceFile round-trip

    #[test]
    fn workspace_file_round_trip() {
        let ws = WorkspaceFile {
            workspaces: vec![WorkspaceSnapshot {
                id: "ws-1".to_string(),
                project_path: "/project".to_string(),
                project_name: "my-project".to_string(),
                terminal_tabs: vec![TerminalTabSnapshot {
                    id: "tab-1".to_string(),
                    label: "Shell".to_string(),
                    split: "horizontal".to_string(),
                    panes: vec![TerminalPaneSnapshot {
                        id: "pane-1".to_string(),
                        startup_command: None,
                        session_type: None,
                        claude_session_id: None,
                    }],
                    session_type: None,
                }],
                active_terminal_tab_id: "tab-1".to_string(),
                worktree_path: None,
                branch: None,
            }],
            selected_id: Some("ws-1".to_string()),
        };
        let json = serde_json::to_string(&ws).unwrap();
        let deserialized: WorkspaceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.workspaces.len(), 1);
        assert_eq!(deserialized.workspaces[0].id, "ws-1");
        assert_eq!(deserialized.selected_id, Some("ws-1".to_string()));
    }

    // DiscoveredClaudeSession round-trip

    #[test]
    fn discovered_claude_session_round_trip() {
        let session = DiscoveredClaudeSession {
            session_id: "abc123".to_string(),
            label: "Fix the bug".to_string(),
            timestamp: "2025-01-15T10:30:00Z".to_string(),
            last_message_role: Some("assistant".to_string()),
        };
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"sessionId\""));
        assert!(json.contains("\"lastMessageRole\""));
        let deserialized: DiscoveredClaudeSession = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, "abc123");
        assert_eq!(deserialized.last_message_role, Some("assistant".to_string()));
    }

    // WorktreeCopyOptions default

    #[test]
    fn worktree_copy_options_default() {
        let opts = WorktreeCopyOptions::default();
        assert!(opts.ai_config);
        assert!(opts.env_files);
    }

    #[test]
    fn workbench_settings_default_terminal_fields() {
        let settings = WorkbenchSettings::default();
        assert_eq!(settings.terminal_performance_mode, "auto");
        assert!(!settings.terminal_telemetry_enabled);
    }

    #[test]
    fn workbench_settings_deserialize_defaults_terminal_fields_when_missing() {
        let json = r#"{"worktreeStrategy":"sibling","trelloEnabled":false,"agentActions":[]}"#;
        let settings: WorkbenchSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.terminal_performance_mode, "auto");
        assert!(!settings.terminal_telemetry_enabled);
    }
}
