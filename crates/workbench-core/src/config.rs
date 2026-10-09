/// Project and settings persistence (reads/writes ~/.workbench/).
use anyhow::Result;
use std::path::PathBuf;

use crate::paths;
use crate::types::{ProjectConfig, ProjectsFile, WorkbenchSettings};

fn config_path() -> PathBuf {
    paths::workbench_config_dir().join("projects.json")
}

pub fn load_projects() -> Result<Vec<ProjectConfig>> {
    let file: ProjectsFile =
        paths::load_json_strict(&config_path(), ProjectsFile { projects: vec![] })?;
    Ok(file.projects)
}

pub fn save_projects(projects: &[ProjectConfig]) -> Result<()> {
    let file = ProjectsFile {
        projects: projects.to_vec(),
    };
    paths::save_json(&config_path(), &file)
}

fn settings_path() -> PathBuf {
    paths::workbench_config_dir().join("settings.json")
}

pub fn load_workbench_settings() -> Result<WorkbenchSettings> {
    paths::load_json_strict(&settings_path(), WorkbenchSettings::default())
}

pub fn save_workbench_settings(settings: &WorkbenchSettings) -> Result<()> {
    paths::save_json(&settings_path(), settings)
}

#[cfg(test)]
mod tests {
    use crate::types::{ProjectConfig, ProjectTask, ProjectsFile};

    #[test]
    fn projects_file_round_trip() {
        let projects = ProjectsFile {
            projects: vec![ProjectConfig {
                name: "test-project".into(),
                path: "/Users/jake/test-project".into(),
                group: None,
                shell: Some("/bin/zsh".into()),
                startup_command: Some("echo hello".into()),
                tasks: vec![ProjectTask {
                    name: "build".into(),
                    command: "cargo build".into(),
                }],
                claude_account_id: None,
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
            group: None,
            shell: None,
            startup_command: Some("npm start".into()),
            tasks: vec![],
            claude_account_id: None,
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
            group: None,
            shell: None,
            startup_command: None,
            tasks: vec![],
            claude_account_id: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("tasks"));
    }
}
