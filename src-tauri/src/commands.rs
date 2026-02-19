use std::collections::HashSet;

use tauri::{AppHandle, Emitter, State};

use crate::claude_sessions;
use crate::codex_config;
use crate::codex_sessions;
use crate::config;
use crate::git;
use crate::github;
use crate::github_poller::GitHubPoller;
use crate::git_watcher::GitWatcher;
use crate::hook_bridge::HookBridgeState;
use crate::pty::PtyManager;
use crate::settings;
use crate::types::GitHubProjectStatusEvent;
use crate::types::{
    BranchInfo, CreateTerminalRequest, CreateTerminalResponse, CreateWorktreeRequest,
    DiscoveredClaudeSession, GitHubRemote, GitInfo, HookScriptInfo, IntegrationStatus, PluginInfo,
    ProjectConfig, SkillInfo, WorkbenchSettings, WorkspaceFile, WorktreeInfo,
};

#[tauri::command]
pub fn list_projects() -> Result<Vec<ProjectConfig>, String> {
    config::load_projects().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_projects(projects: Vec<ProjectConfig>) -> Result<bool, String> {
    config::save_projects(&projects).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn create_terminal(
    request: CreateTerminalRequest,
    pty_manager: State<'_, PtyManager>,
    hook_bridge: State<'_, HookBridgeState>,
    app_handle: tauri::AppHandle,
) -> Result<CreateTerminalResponse, String> {
    pty_manager
        .spawn(
            request.id.clone(),
            request.project_path,
            request.shell,
            request.cols,
            request.rows,
            request.startup_command,
            hook_bridge.socket_path().map(str::to_string),
            app_handle,
        )
        .map_err(|e| e.to_string())?;

    Ok(CreateTerminalResponse {
        id: request.id,
        backend: "pty".to_string(),
    })
}

#[tauri::command]
pub fn write_terminal(
    session_id: String,
    data: String,
    pty_manager: State<'_, PtyManager>,
) -> Result<bool, String> {
    pty_manager
        .write(&session_id, &data)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn resize_terminal(
    session_id: String,
    cols: u16,
    rows: u16,
    pty_manager: State<'_, PtyManager>,
) -> Result<bool, String> {
    pty_manager
        .resize(&session_id, cols, rows)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn kill_terminal(
    session_id: String,
    pty_manager: State<'_, PtyManager>,
    app_handle: tauri::AppHandle,
) -> Result<bool, String> {
    pty_manager
        .kill(&session_id, &app_handle)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn open_in_vscode(path: String) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        // Use `open -a` which works regardless of PATH (Tauri .app doesn't inherit shell PATH)
        std::process::Command::new("open")
            .args(["-a", "Visual Studio Code", &path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        // VS Code installs `code.cmd` — launching via cmd /c finds it on PATH
        std::process::Command::new("cmd")
            .args(["/c", "code", &path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("code")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(true)
}

#[tauri::command]
pub fn load_workspaces(git_watcher: State<'_, GitWatcher>) -> Result<WorkspaceFile, String> {
    let snapshot = config::load_workspaces().map_err(|e| e.to_string())?;
    git_watcher.sync_projects(workspace_project_paths(&snapshot));
    Ok(snapshot)
}

#[tauri::command]
pub fn save_workspaces(
    snapshot: WorkspaceFile,
    git_watcher: State<'_, GitWatcher>,
) -> Result<bool, String> {
    config::save_workspaces(&snapshot).map_err(|e| e.to_string())?;
    git_watcher.sync_projects(workspace_project_paths(&snapshot));
    Ok(true)
}

#[tauri::command]
pub fn discover_claude_sessions(
    project_path: String,
) -> Result<Vec<DiscoveredClaudeSession>, String> {
    claude_sessions::discover_claude_sessions(&project_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_claude_settings(
    scope: String,
    project_path: Option<String>,
) -> Result<serde_json::Value, String> {
    settings::load_settings(&scope, project_path.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_claude_settings(
    scope: String,
    project_path: Option<String>,
    value: serde_json::Value,
) -> Result<bool, String> {
    settings::save_settings(&scope, project_path.as_deref(), &value).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn list_claude_plugins() -> Result<Vec<PluginInfo>, String> {
    settings::list_plugins().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_claude_skills() -> Result<Vec<SkillInfo>, String> {
    settings::list_skills().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_claude_hooks_scripts() -> Result<Vec<HookScriptInfo>, String> {
    settings::list_hooks_scripts().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_info(path: String) -> Result<GitInfo, String> {
    git::git_info(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_worktrees(path: String) -> Result<Vec<WorktreeInfo>, String> {
    git::list_worktrees(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_worktree(request: CreateWorktreeRequest) -> Result<String, String> {
    git::create_worktree(&request).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn remove_worktree(
    repo_path: String,
    worktree_path: String,
    force: bool,
) -> Result<bool, String> {
    git::remove_worktree(&repo_path, &worktree_path, force).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn list_branches(path: String) -> Result<Vec<BranchInfo>, String> {
    git::list_branches(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn discover_codex_sessions(
    project_path: String,
) -> Result<Vec<DiscoveredClaudeSession>, String> {
    codex_sessions::discover_codex_sessions(&project_path).map_err(|e| e.to_string())
}

// Workbench settings commands

#[tauri::command]
pub fn load_workbench_settings() -> Result<WorkbenchSettings, String> {
    config::load_workbench_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_workbench_settings(settings: WorkbenchSettings) -> Result<bool, String> {
    config::save_workbench_settings(&settings).map_err(|e| e.to_string())?;
    Ok(true)
}

// GitHub integration commands

#[tauri::command(async)]
pub fn github_is_available() -> bool {
    github::is_gh_available()
}

#[tauri::command(async)]
pub fn github_get_remote(path: String) -> Result<GitHubRemote, String> {
    github::get_github_remote(&path).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn github_set_tracked_projects(
    project_paths: Vec<String>,
    poller: State<'_, GitHubPoller>,
) -> Result<bool, String> {
    poller.set_tracked_projects(project_paths);
    Ok(true)
}

fn emit_github_status(app_handle: &AppHandle, project_path: &str) {
    let status = github::get_project_status(project_path);
    let _ = app_handle.emit(
        "github:project-status",
        GitHubProjectStatusEvent {
            project_path: project_path.to_string(),
            status,
        },
    );
}

#[tauri::command(async)]
pub fn github_refresh_project(
    project_path: String,
    poller: State<'_, GitHubPoller>,
) -> Result<bool, String> {
    poller.request_refresh(project_path);
    Ok(true)
}

#[tauri::command(async)]
pub fn github_update_pr_branch(
    project_path: String,
    pr_number: u64,
    app_handle: AppHandle,
) -> Result<bool, String> {
    github::update_pr_branch(&project_path, pr_number).map_err(|e| e.to_string())?;
    emit_github_status(&app_handle, &project_path);
    Ok(true)
}

#[tauri::command(async)]
pub fn github_rerun_workflow(
    project_path: String,
    run_id: u64,
) -> Result<bool, String> {
    github::rerun_workflow(&project_path, run_id).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub fn github_mark_pr_ready(
    project_path: String,
    pr_number: u64,
    app_handle: AppHandle,
) -> Result<bool, String> {
    github::mark_pr_ready(&project_path, pr_number).map_err(|e| e.to_string())?;
    emit_github_status(&app_handle, &project_path);
    Ok(true)
}

#[tauri::command(async)]
pub fn github_merge_pr(
    project_path: String,
    pr_number: u64,
    app_handle: AppHandle,
) -> Result<bool, String> {
    github::merge_pr(&project_path, pr_number).map_err(|e| e.to_string())?;
    emit_github_status(&app_handle, &project_path);
    Ok(true)
}

#[tauri::command(async)]
pub fn delete_branch(
    repo_path: String,
    branch: String,
    force: bool,
) -> Result<bool, String> {
    git::delete_branch(&repo_path, &branch, force).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn open_url(url: String) -> Result<bool, String> {
    github::open_url(&url).map_err(|e| e.to_string())?;
    Ok(true)
}

// Integration check/apply commands

#[tauri::command]
pub fn check_claude_integration() -> IntegrationStatus {
    settings::check_workbench_hook_integration()
}

#[tauri::command]
pub fn check_codex_integration() -> IntegrationStatus {
    codex_config::check_codex_config_status()
}

#[tauri::command]
pub fn apply_claude_integration() -> Result<bool, String> {
    settings::ensure_workbench_hook_integration().map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn apply_codex_integration() -> Result<bool, String> {
    codex_config::ensure_codex_config().map_err(|e| e.to_string())?;
    Ok(true)
}

fn workspace_project_paths(snapshot: &WorkspaceFile) -> Vec<String> {
    snapshot
        .workspaces
        .iter()
        .map(|ws| ws.project_path.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::types::{WorkspaceFile, WorkspaceSnapshot};

    use super::workspace_project_paths;

    fn make_workspace(id: &str, project_path: &str) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            id: id.to_string(),
            project_path: project_path.to_string(),
            project_name: format!("project-{id}"),
            terminal_tabs: vec![],
            active_terminal_tab_id: String::new(),
            worktree_path: None,
            branch: None,
        }
    }

    #[test]
    fn workspace_project_paths_dedupes_project_paths() {
        let snapshot = WorkspaceFile {
            workspaces: vec![
                make_workspace("1", "/repo/a"),
                make_workspace("2", "/repo/a"),
                make_workspace("3", "/repo/b"),
            ],
            selected_id: Some("1".to_string()),
        };

        let paths = workspace_project_paths(&snapshot);
        let path_set: HashSet<String> = paths.into_iter().collect();

        assert_eq!(
            path_set,
            HashSet::from(["/repo/a".to_string(), "/repo/b".to_string()])
        );
    }

    #[test]
    fn workspace_project_paths_empty_snapshot_returns_empty_vec() {
        let snapshot = WorkspaceFile {
            workspaces: vec![],
            selected_id: None,
        };

        let paths = workspace_project_paths(&snapshot);
        assert!(paths.is_empty());
    }
}
