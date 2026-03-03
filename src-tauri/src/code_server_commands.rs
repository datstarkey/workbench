use tauri::State;

use crate::code_server::CodeServerManager;
use crate::types::CodeServerInfo;

#[tauri::command]
pub fn start_code_server(
    session_id: String,
    project_path: String,
    code_server_manager: State<'_, CodeServerManager>,
    app_handle: tauri::AppHandle,
) -> Result<CodeServerInfo, String> {
    code_server_manager
        .start(session_id, project_path, app_handle)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stop_code_server(
    session_id: String,
    code_server_manager: State<'_, CodeServerManager>,
) -> Result<bool, String> {
    code_server_manager
        .stop(&session_id)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn detect_code_server(
    code_server_manager: State<'_, CodeServerManager>,
) -> Result<String, String> {
    code_server_manager.detect().map_err(|e| e.to_string())
}
