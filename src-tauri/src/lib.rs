mod claude_sessions;
mod codex;
mod commands;
mod config;
mod git;
mod git_watcher;
mod hook_bridge;
mod paths;
mod pty;
mod session_utils;
mod settings;
mod types;

use git_watcher::GitWatcher;
use hook_bridge::HookBridgeState;
use pty::PtyManager;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(PtyManager::new())
        .setup(|app| {
            let handle = app.handle().clone();
            let bridge = HookBridgeState::new(handle.clone());
            app.manage(bridge);
            let git_watcher = GitWatcher::new(handle);
            app.manage(git_watcher);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::save_projects,
            commands::create_terminal,
            commands::write_terminal,
            commands::resize_terminal,
            commands::kill_terminal,
            commands::open_in_vscode,
            commands::load_workspaces,
            commands::save_workspaces,
            commands::discover_claude_sessions,
            commands::load_claude_settings,
            commands::save_claude_settings,
            commands::list_claude_plugins,
            commands::list_claude_skills,
            commands::list_claude_hooks_scripts,
            commands::git_info,
            commands::list_worktrees,
            commands::create_worktree,
            commands::remove_worktree,
            commands::list_branches,
            commands::discover_codex_sessions,
            commands::watch_project,
            commands::unwatch_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Workbench");
}
