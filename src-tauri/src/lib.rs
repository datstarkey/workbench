mod claude_sessions;
mod codex;
mod commands;
mod config;
mod git;
mod git_watcher;
mod github;
mod hook_bridge;
mod menu;
mod paths;
mod pty;
mod session_utils;
mod settings;
mod trello;
mod trello_commands;
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
            menu::build(&handle).expect("failed to build menu");
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
            commands::load_workbench_settings,
            commands::save_workbench_settings,
            commands::watch_project,
            commands::unwatch_project,
            commands::github_is_available,
            commands::github_get_remote,
            commands::github_project_status,
            commands::github_pr_checks,
            commands::open_url,
            trello_commands::trello_validate_auth,
            trello_commands::trello_list_boards,
            trello_commands::trello_fetch_board_data,
            trello_commands::trello_list_columns,
            trello_commands::trello_list_labels,
            trello_commands::trello_create_card,
            trello_commands::trello_move_card,
            trello_commands::trello_add_label,
            trello_commands::trello_remove_label,
            trello_commands::trello_load_credentials,
            trello_commands::trello_save_credentials,
            trello_commands::trello_disconnect,
            trello_commands::trello_load_project_config,
            trello_commands::trello_save_project_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Workbench");
}
