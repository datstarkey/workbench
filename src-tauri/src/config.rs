/// Project and workspace persistence (reads/writes ~/.workbench/).
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

use crate::paths;
use crate::types::{ProjectConfig, ProjectsFile, WorkspaceFile};

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
