use tauri::State;

use crate::config;
use crate::pty::PtyManager;
use crate::settings;
use crate::types::{
    ClaudeSessionsFile, CreateTerminalRequest, CreateTerminalResponse, DiscoveredClaudeSession,
    HookScriptInfo, PluginInfo, ProjectConfig, SkillInfo, WorkspaceFile,
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
    std::process::Command::new("code")
        .arg(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
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
pub fn load_claude_sessions() -> Result<ClaudeSessionsFile, String> {
    config::load_claude_sessions().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_claude_sessions(file: ClaudeSessionsFile) -> Result<bool, String> {
    config::save_claude_sessions(&file).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn discover_claude_sessions(
    project_path: String,
) -> Result<Vec<DiscoveredClaudeSession>, String> {
    config::discover_claude_sessions(&project_path).map_err(|e| e.to_string())
}

// Claude Code settings commands

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
