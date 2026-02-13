use tauri::State;

use crate::claude_sessions;
use crate::codex;
use crate::config;
use crate::git;
use crate::github;
use crate::git_watcher::GitWatcher;
use crate::hook_bridge::HookBridgeState;
use crate::pty::PtyManager;
use crate::settings;
use crate::types::{
    BranchInfo, CreateTerminalRequest, CreateTerminalResponse, CreateWorktreeRequest,
    DiscoveredClaudeSession, GitHubCheckDetail, GitHubProjectStatus, GitHubRemote, GitInfo,
    HookScriptInfo, PluginInfo, ProjectConfig, SkillInfo, WorkspaceFile, WorktreeInfo,
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
    let startup = request.startup_command.as_deref().map(str::trim_start);
    let is_claude_start = startup.is_some_and(|cmd| {
        cmd == "claude" || cmd.starts_with("claude ") || cmd.starts_with("claude\n")
    });
    let is_codex_start = startup.is_some_and(|cmd| {
        cmd == "codex" || cmd.starts_with("codex ") || cmd.starts_with("codex\n")
    });
    if is_claude_start {
        settings::ensure_workbench_hook_integration().map_err(|e| e.to_string())?;
    }
    if is_codex_start {
        codex::ensure_codex_config().map_err(|e| e.to_string())?;
    }

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
    #[cfg(not(target_os = "macos"))]
    {
        std::process::Command::new("code")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(true)
}

#[tauri::command]
pub fn load_workspaces() -> Result<WorkspaceFile, String> {
    config::load_workspaces().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_workspaces(snapshot: WorkspaceFile) -> Result<bool, String> {
    config::save_workspaces(&snapshot).map_err(|e| e.to_string())?;
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

#[tauri::command]
pub fn remove_worktree(repo_path: String, worktree_path: String) -> Result<bool, String> {
    git::remove_worktree(&repo_path, &worktree_path).map_err(|e| e.to_string())?;
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
    codex::discover_codex_sessions(&project_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn watch_project(path: String, state: State<'_, GitWatcher>) -> Result<bool, String> {
    state.watch_project(&path).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn unwatch_project(path: String, state: State<'_, GitWatcher>) -> Result<bool, String> {
    state.unwatch_project(&path).map_err(|e| e.to_string())?;
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
pub fn github_project_status(project_path: String) -> GitHubProjectStatus {
    github::get_project_status(&project_path)
}

#[tauri::command(async)]
pub fn github_pr_checks(
    project_path: String,
    pr_number: u64,
) -> Result<Vec<GitHubCheckDetail>, String> {
    github::list_pr_checks(&project_path, pr_number).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_url(url: String) -> Result<bool, String> {
    github::open_url(&url).map_err(|e| e.to_string())?;
    Ok(true)
}
