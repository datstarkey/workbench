/// Project and workspace persistence (reads/writes ~/.workbench/).
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

use crate::paths;
use crate::types::{ProjectConfig, ProjectsFile, WorkbenchSettings, WorkspaceFile};

fn config_path() -> PathBuf {
    paths::workbench_config_dir().join("projects.json")
}

pub fn load_projects() -> Result<Vec<ProjectConfig>> {
    let path = config_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&path)?;
    let file: ProjectsFile = serde_json::from_str(&content)?;
    Ok(file.projects)
}

pub fn save_projects(projects: &[ProjectConfig]) -> Result<()> {
    let file = ProjectsFile {
        projects: projects.to_vec(),
    };
    let content = serde_json::to_string_pretty(&file)?;
    paths::atomic_write(&config_path(), &content)?;
    Ok(())
}

fn workspace_path() -> PathBuf {
    paths::workbench_config_dir().join("workspaces.json")
}

pub fn load_workspaces() -> Result<WorkspaceFile> {
    let path = workspace_path();
    if !path.exists() {
        return Ok(WorkspaceFile {
            workspaces: Vec::new(),
            selected_id: None,
        });
    }

    let content = fs::read_to_string(&path)?;
    let file: WorkspaceFile = serde_json::from_str(&content)?;
    Ok(file)
}

pub fn save_workspaces(file: &WorkspaceFile) -> Result<()> {
    let content = serde_json::to_string_pretty(file)?;
    paths::atomic_write(&workspace_path(), &content)?;
    Ok(())
}

fn settings_path() -> PathBuf {
    paths::workbench_config_dir().join("settings.json")
}

pub fn load_workbench_settings() -> Result<WorkbenchSettings> {
    let path = settings_path();
    if !path.exists() {
        return Ok(WorkbenchSettings::default());
    }
    let content = fs::read_to_string(&path)?;
    let settings: WorkbenchSettings = serde_json::from_str(&content)?;
    Ok(settings)
}

pub fn save_workbench_settings(settings: &WorkbenchSettings) -> Result<()> {
    let content = serde_json::to_string_pretty(settings)?;
    paths::atomic_write(&settings_path(), &content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::types::{
        ProjectConfig, ProjectTask, ProjectsFile, TerminalPaneSnapshot, TerminalTabSnapshot,
        WorkspaceFile, WorkspaceSnapshot,
    };

    #[test]
    fn projects_file_round_trip() {
        let projects = ProjectsFile {
            projects: vec![ProjectConfig {
                name: "test-project".into(),
                path: "/Users/jake/test-project".into(),
                shell: Some("/bin/zsh".into()),
                startup_command: Some("echo hello".into()),
                tasks: vec![ProjectTask {
                    name: "build".into(),
                    command: "cargo build".into(),
                }],
            }],
        };
        let json = serde_json::to_string(&projects).unwrap();
        let parsed: ProjectsFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.projects.len(), 1);
        assert_eq!(parsed.projects[0].name, "test-project");
        assert_eq!(parsed.projects[0].path, "/Users/jake/test-project");
        assert_eq!(parsed.projects[0].shell, Some("/bin/zsh".to_string()));
        assert_eq!(
            parsed.projects[0].startup_command,
            Some("echo hello".to_string())
        );
        assert_eq!(parsed.projects[0].tasks.len(), 1);
        assert_eq!(parsed.projects[0].tasks[0].name, "build");
    }

    #[test]
    fn project_config_optional_fields_omitted() {
        let json = r#"{"name":"minimal","path":"/tmp/minimal"}"#;
        let parsed: ProjectConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.name, "minimal");
        assert_eq!(parsed.path, "/tmp/minimal");
        assert!(parsed.shell.is_none());
        assert!(parsed.startup_command.is_none());
        assert!(parsed.tasks.is_empty());
    }

    #[test]
    fn project_config_camel_case_serialization() {
        let config = ProjectConfig {
            name: "test".into(),
            path: "/test".into(),
            shell: None,
            startup_command: Some("npm start".into()),
            tasks: vec![],
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("startupCommand"));
        assert!(!json.contains("startup_command"));
        // shell should be omitted (skip_serializing_if = "Option::is_none")
        assert!(!json.contains("shell"));
    }

    #[test]
    fn project_config_omits_empty_tasks() {
        let config = ProjectConfig {
            name: "test".into(),
            path: "/test".into(),
            shell: None,
            startup_command: None,
            tasks: vec![],
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("tasks"));
    }

    #[test]
    fn workspace_file_empty_workspaces() {
        let file = WorkspaceFile {
            workspaces: Vec::new(),
            selected_id: None,
        };
        let json = serde_json::to_string(&file).unwrap();
        let parsed: WorkspaceFile = serde_json::from_str(&json).unwrap();
        assert!(parsed.workspaces.is_empty());
        assert!(parsed.selected_id.is_none());
    }

    #[test]
    fn workspace_file_with_populated_snapshots() {
        let file = WorkspaceFile {
            workspaces: vec![WorkspaceSnapshot {
                id: "ws-1".into(),
                project_path: "/Users/jake/project".into(),
                project_name: "project".into(),
                terminal_tabs: vec![TerminalTabSnapshot {
                    id: "tab-1".into(),
                    label: "Terminal".into(),
                    split: "horizontal".into(),
                    panes: vec![TerminalPaneSnapshot {
                        id: "pane-1".into(),
                        startup_command: Some("cargo test".into()),
                        session_type: Some("claude".into()),
                        claude_session_id: Some("sess-123".into()),
                    }],
                    session_type: None,
                }],
                active_terminal_tab_id: "tab-1".into(),
                worktree_path: Some("/Users/jake/project-wt".into()),
                branch: Some("feature/test".into()),
            }],
            selected_id: Some("ws-1".into()),
        };
        let json = serde_json::to_string_pretty(&file).unwrap();
        let parsed: WorkspaceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.workspaces.len(), 1);
        assert_eq!(parsed.selected_id, Some("ws-1".to_string()));

        let ws = &parsed.workspaces[0];
        assert_eq!(ws.project_name, "project");
        assert_eq!(ws.terminal_tabs.len(), 1);
        assert_eq!(ws.terminal_tabs[0].panes.len(), 1);
        assert_eq!(
            ws.terminal_tabs[0].panes[0].startup_command,
            Some("cargo test".to_string())
        );
        assert_eq!(ws.worktree_path, Some("/Users/jake/project-wt".to_string()));
        assert_eq!(ws.branch, Some("feature/test".to_string()));
    }

    #[test]
    fn workspace_file_camel_case_keys() {
        let file = WorkspaceFile {
            workspaces: vec![WorkspaceSnapshot {
                id: "ws-1".into(),
                project_path: "/test".into(),
                project_name: "test".into(),
                terminal_tabs: vec![],
                active_terminal_tab_id: "tab-1".into(),
                worktree_path: None,
                branch: None,
            }],
            selected_id: Some("ws-1".into()),
        };
        let json = serde_json::to_string(&file).unwrap();
        assert!(json.contains("projectPath"));
        assert!(!json.contains("project_path"));
        assert!(json.contains("projectName"));
        assert!(!json.contains("project_name"));
        assert!(json.contains("terminalTabs"));
        assert!(!json.contains("terminal_tabs"));
        assert!(json.contains("activeTerminalTabId"));
        assert!(!json.contains("active_terminal_tab_id"));
        assert!(json.contains("selectedId"));
        assert!(!json.contains("selected_id"));
    }
}
