use std::collections::HashSet;

use tauri::{AppHandle, Emitter, State};

use crate::claude_accounts;
use crate::claude_sessions;
use crate::codex_config;
use crate::codex_sessions;
use crate::config;
use crate::git;
use crate::git_watcher::GitWatcher;
use crate::github;
use crate::github_poller::GitHubPoller;
use crate::hook_bridge::{HookBridgeState, HookLogEntry};
use crate::package_scripts;
use crate::sandbox_runtime;
use crate::settings;
use crate::types::GitHubProjectStatusEvent;
use crate::types::{
    BranchInfo, CreateWorktreeRequest, DiscoveredClaudeSession, GitHubRemote, GitHubRepo, GitInfo,
    HookScriptInfo, IntegrationStatus, PackageInfo, PluginInfo, ProjectConfig, SkillInfo,
    WorkbenchSettings, WorkspaceFile, WorktreeInfo,
};

/// Read a file dropped onto a chat (image, PDF or text) so it can be attached to the message.
#[tauri::command]
pub async fn read_chat_attachment(
    path: String,
) -> Result<workbench_core::chat_attachment::ChatAttachment, String> {
    tauri::async_runtime::spawn_blocking(move || {
        workbench_core::chat_attachment::read(std::path::Path::new(&path))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_projects() -> Result<Vec<ProjectConfig>, String> {
    config::load_projects().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_projects(
    projects: Vec<ProjectConfig>,
    hook_bridge: State<'_, HookBridgeState>,
) -> Result<bool, String> {
    config::save_projects(&projects).map_err(|e| e.to_string())?;
    // Project roots are the sandbox's writable set, so a newly added project has
    // to reach the srt settings file before its first Claude launch.
    refresh_sandbox_runtime_settings(None, &hook_bridge);
    Ok(true)
}

#[tauri::command]
pub fn open_in_vscode(path: String) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        // Use `open -a` which works regardless of PATH (Tauri .app doesn't inherit shell PATH)
        crate::shell::spawn_detached(crate::shell::command("open").args([
            "-a",
            "Visual Studio Code",
            &path,
        ]))
        .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        // VS Code installs `code.cmd` — launching via cmd /c finds it on PATH
        crate::shell::spawn_detached(crate::shell::command("cmd").args(["/c", "code", &path]))
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        crate::shell::spawn_detached(crate::shell::command("code").arg(&path))
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

// Async: it reads every session file of every account, off the main thread.
#[tauri::command]
pub async fn discover_claude_sessions(
    project_path: String,
) -> Result<Vec<DiscoveredClaudeSession>, String> {
    crate::blocking(move || {
        claude_sessions::discover_claude_sessions(&project_path).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn claude_auth_status(
    account_id: Option<String>,
) -> Result<claude_accounts::ClaudeAuthStatus, String> {
    crate::blocking(move || {
        claude_accounts::auth_status(account_id.as_deref()).map_err(|e| e.to_string())
    })
    .await
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
pub async fn git_info(path: String) -> Result<GitInfo, String> {
    crate::blocking(move || git::git_info(&path).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn list_worktrees(path: String) -> Result<Vec<WorktreeInfo>, String> {
    crate::blocking(move || git::list_worktrees(&path).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn create_worktree(request: CreateWorktreeRequest) -> Result<String, String> {
    crate::blocking(move || git::create_worktree(&request).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn remove_worktree(
    repo_path: String,
    worktree_path: String,
    force: bool,
) -> Result<bool, String> {
    crate::blocking(move || {
        git::remove_worktree(&repo_path, &worktree_path, force).map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn list_branches(path: String) -> Result<Vec<BranchInfo>, String> {
    crate::blocking(move || git::list_branches(&path).map_err(|e| e.to_string())).await
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
pub fn save_workbench_settings(
    mut settings: WorkbenchSettings,
    hook_bridge: State<'_, HookBridgeState>,
) -> Result<bool, String> {
    // Each window holds its own settings store; one loaded before the LAN token
    // was generated must not wipe it (and lock paired phones out) on save.
    if settings.server_token.is_none() {
        settings.server_token = config::load_workbench_settings()
            .ok()
            .and_then(|s| s.server_token);
    }
    config::save_workbench_settings(&settings).map_err(|e| e.to_string())?;
    refresh_sandbox_runtime_settings(Some(&settings), &hook_bridge);
    Ok(true)
}

/// Regenerate the `srt` settings file and return its absolute path.
///
/// Writes rather than just resolving the path, and fails loudly: the frontend
/// wraps launch commands with whatever path this returns, so handing back a path
/// to a file that does not exist would produce a `claude` command srt refuses to
/// run. On `Err` the frontend launches unwrapped instead.
#[tauri::command]
pub fn sandbox_runtime_settings_path(
    hook_bridge: State<'_, HookBridgeState>,
) -> Result<String, String> {
    let settings = config::load_workbench_settings().map_err(|e| e.to_string())?;
    let projects = config::load_projects().map_err(|e| e.to_string())?;
    let path = sandbox_runtime::write_settings(&settings, &projects, hook_bridge.socket_path())
        .map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// Regenerate `~/.workbench/sandbox-runtime.json` from the current settings,
/// the registered projects, and the live hook-bridge port.
///
/// Pass `settings` when the caller already has them (a save that has not been
/// re-read yet); otherwise they are loaded from disk. Failures are logged, not
/// propagated — a stale sandbox file must not fail a settings save, and
/// `sandbox_runtime_settings_path` is the path that gates actual wrapping.
pub fn refresh_sandbox_runtime_settings(
    settings: Option<&WorkbenchSettings>,
    hook_bridge: &HookBridgeState,
) {
    let loaded;
    let settings = match settings {
        Some(s) => s,
        None => match config::load_workbench_settings() {
            Ok(s) => {
                loaded = s;
                &loaded
            }
            Err(e) => {
                log::warn!("[sandbox-runtime] Failed to load settings: {e}");
                return;
            }
        },
    };

    let projects = config::load_projects().unwrap_or_else(|e| {
        // Degrade to cwd-only writes rather than skipping the file entirely.
        log::warn!("[sandbox-runtime] Failed to load projects: {e}");
        Vec::new()
    });

    if let Err(e) = sandbox_runtime::write_settings(settings, &projects, hook_bridge.socket_path())
    {
        log::warn!("[sandbox-runtime] Failed to write settings file: {e}");
    }
}

// GitHub integration commands

#[tauri::command]
pub async fn github_is_available() -> bool {
    crate::blocking(move || github::is_gh_available()).await
}

#[tauri::command]
pub async fn github_get_remote(path: String) -> Option<GitHubRemote> {
    crate::blocking(move || github::get_github_remote(&path).ok()).await
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

#[tauri::command]
pub async fn github_refresh_project(
    project_path: String,
    app_handle: AppHandle,
    poller: State<'_, GitHubPoller>,
) -> Result<bool, String> {
    poller.defer_project(&project_path);
    crate::blocking(move || emit_github_status(&app_handle, &project_path)).await;
    Ok(true)
}

#[tauri::command]
pub async fn github_update_pr_branch(
    project_path: String,
    pr_number: u64,
    app_handle: AppHandle,
) -> Result<bool, String> {
    crate::blocking(move || {
        github::update_pr_branch(&project_path, pr_number).map_err(|e| e.to_string())?;
        emit_github_status(&app_handle, &project_path);
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn github_rerun_workflow(project_path: String, run_id: u64) -> Result<bool, String> {
    crate::blocking(move || {
        github::rerun_workflow(&project_path, run_id).map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn github_mark_pr_ready(
    project_path: String,
    pr_number: u64,
    app_handle: AppHandle,
) -> Result<bool, String> {
    crate::blocking(move || {
        github::mark_pr_ready(&project_path, pr_number).map_err(|e| e.to_string())?;
        emit_github_status(&app_handle, &project_path);
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn github_merge_pr(
    project_path: String,
    pr_number: u64,
    options: crate::types::MergePrOptions,
    app_handle: AppHandle,
) -> Result<bool, String> {
    crate::blocking(move || {
        github::merge_pr(&project_path, pr_number, &options).map_err(|e| e.to_string())?;
        emit_github_status(&app_handle, &project_path);
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn delete_branch(repo_path: String, branch: String, force: bool) -> Result<bool, String> {
    crate::blocking(move || {
        git::delete_branch(&repo_path, &branch, force).map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
}

#[tauri::command]
pub fn open_url(url: String) -> Result<bool, String> {
    crate::shell::open_url(&url).map_err(|e| e.to_string())?;
    Ok(true)
}

// GitHub clone + PR actions

#[tauri::command]
pub async fn github_list_repos() -> Result<Vec<GitHubRepo>, String> {
    crate::blocking(move || github::list_repos().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn github_checkout_pr(project_path: String, pr_number: u64) -> Result<(), String> {
    crate::blocking(move || {
        github::checkout_pr(&project_path, pr_number).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn github_fetch_pr_branch(project_path: String, branch: String) -> Result<(), String> {
    crate::blocking(move || {
        github::fetch_pr_branch(&project_path, &branch).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn clone_repo(url: String, dest_path: String) -> Result<(), String> {
    crate::blocking(move || git::clone_repo(&url, &dest_path).map_err(|e| e.to_string())).await
}

// Integration check/apply commands

#[tauri::command]
pub async fn codex_supports_no_daemon() -> bool {
    tauri::async_runtime::spawn_blocking(codex_config::supports_no_daemon)
        .await
        .unwrap_or(false)
}

#[tauri::command]
pub fn check_codex_integration() -> IntegrationStatus {
    codex_config::check_codex_config_status()
}

#[tauri::command]
pub fn apply_codex_integration() -> Result<bool, String> {
    codex_config::ensure_codex_config().map_err(|e| e.to_string())?;
    Ok(true)
}

// Hook bridge log commands

#[tauri::command]
pub fn get_hook_logs(hook_bridge: State<'_, HookBridgeState>) -> Result<Vec<HookLogEntry>, String> {
    Ok(hook_bridge.get_logs())
}

/// The hook-bridge socket address (`127.0.0.1:<port>`) the frontend forwards to
/// the embedded server so server-hosted xterm panes set `WORKBENCH_HOOK_SOCKET`
/// and the Claude/Codex hook bridge fires for them — parity with local PTYs.
#[tauri::command]
pub fn terminal_hook_socket(hook_bridge: State<'_, HookBridgeState>) -> Option<String> {
    hook_bridge.socket_path().map(str::to_string)
}

#[tauri::command]
pub fn clear_hook_logs(hook_bridge: State<'_, HookBridgeState>) -> Result<(), String> {
    hook_bridge.clear_logs();
    Ok(())
}

// Native terminal availability check

#[tauri::command]
pub async fn is_native_terminal_available() -> bool {
    crate::blocking(move || cfg!(target_os = "macos")).await
}

#[tauri::command]
pub async fn get_package_info(path: String) -> Result<Option<PackageInfo>, String> {
    crate::blocking(move || {
        package_scripts::read(std::path::Path::new(&path)).map_err(|e| e.to_string())
    })
    .await
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
            split_view: None,
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
