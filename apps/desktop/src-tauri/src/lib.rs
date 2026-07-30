// Pure logic now lives in `workbench-core`. Re-export each moved module at the
// crate root so existing `crate::config`, `crate::git`, `crate::types`, … paths
// throughout the desktop crate keep resolving without per-file edits.
pub use workbench_core::{
    claude_sessions, codex_config, codex_sessions, config, git, github, paths, session_utils,
    settings, shell_integration, trello, trello_automation, types,
};

mod commands;
mod git_commands;
mod git_watcher;
mod github_poller;
mod hook_bridge;
mod menu;
#[cfg(target_os = "macos")]
mod native_terminal;
#[cfg(target_os = "macos")]
mod native_terminal_commands;
mod observability;
mod pty;
mod refresh_dispatcher;
mod server_control;
mod trello_commands;

use git_watcher::GitWatcher;
use github_poller::GitHubPoller;
use hook_bridge::HookBridgeState;
use pty::PtyManager;
use refresh_dispatcher::RefreshDispatcher;
use tauri::Manager;

/// Build the invoke handler with all shared commands, plus native terminal
/// commands on macOS. Uses a declarative macro to avoid duplicating the
/// shared command list across cfg branches.
macro_rules! build_invoke_handler {
    ( $( $extra:path ),* $(,)? ) => {
        tauri::generate_handler![
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
            commands::github_is_available,
            commands::github_get_remote,
            commands::github_set_tracked_projects,
            commands::github_refresh_project,
            commands::github_update_pr_branch,
            commands::github_list_pr_checks,
            commands::github_rerun_workflow,
            commands::github_mark_pr_ready,
            commands::github_merge_pr,
            commands::github_list_repos,
            commands::github_checkout_pr,
            commands::github_fetch_pr_branch,
            commands::clone_repo,
            commands::delete_branch,
            commands::open_url,
            commands::check_claude_integration,
            commands::check_codex_integration,
            commands::apply_claude_integration,
            commands::apply_codex_integration,
            commands::get_hook_logs,
            commands::clear_hook_logs,
            commands::terminal_hook_socket,
            commands::is_native_terminal_available,
            git_commands::git_status,
            git_commands::git_log,
            git_commands::git_stage,
            git_commands::git_unstage,
            git_commands::git_commit,
            git_commands::git_checkout_branch,
            git_commands::git_stash_list,
            git_commands::git_stash_push,
            git_commands::git_stash_pop,
            git_commands::git_stash_drop,
            git_commands::git_discard_file,
            git_commands::git_fetch,
            git_commands::git_pull,
            git_commands::git_push,
            git_commands::git_show_files,
            git_commands::git_revert,
            git_commands::git_create_branch,
            git_commands::git_commit_amend,
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
            server_control::start_server,
            server_control::stop_server,
            server_control::server_status,
            server_control::terminal_server_status,
            $( $extra ),*
        ]
    };
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let context = tauri::generate_context!();

    // Initialise backend error reporting before building the app so panics in
    // any thread are captured. No-op in debug builds. Guard kept alive for the
    // whole process — dropping it flushes pending events on shutdown.
    let _sentry_guard = observability::init(context.package_info().version.to_string());

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .manage(PtyManager::new())
        .manage(RefreshDispatcher::new())
        .manage(server_control::ServerControl::new())
        .setup(|app| {
            let handle = app.handle().clone();
            menu::build(&handle).expect("failed to build menu");
            let bridge = HookBridgeState::new(handle.clone());
            app.manage(bridge);
            let git_watcher = GitWatcher::new(handle);
            app.manage(git_watcher);
            let github_poller = GitHubPoller::new(app.handle().clone());
            app.manage(github_poller);

            // Boot the always-on loopback server (127.0.0.1, ephemeral port)
            // synchronously on the Tauri async runtime so it is listening
            // before the webview mounts. `spawn_embedded` binds before
            // returning, so by the time setup() returns the server is ready.
            let sc = app.state::<server_control::ServerControl>();
            match tauri::async_runtime::block_on(workbench_server::spawn_embedded(
                "127.0.0.1",
                0,
                None,
            )) {
                Ok(handle) => sc.set_loopback(handle),
                // Degrade instead of aborting launch: terminals will fail to connect
                // (surfaced per-pane) but the rest of the app still works. A hard
                // panic here would take down the whole window on a transient bind
                // failure (port exhaustion, sandbox), which terminals never used to
                // require.
                Err(e) => log::error!("failed to start loopback embedded server: {e}"),
            }

            Ok(())
        });

    #[cfg(target_os = "macos")]
    {
        builder = builder
            .manage(native_terminal::NativeTerminalManager::new())
            .invoke_handler(build_invoke_handler!(
                native_terminal_commands::create_native_terminal,
                native_terminal_commands::resize_native_terminal,
                native_terminal_commands::set_native_terminal_visible,
                native_terminal_commands::kill_native_terminal,
                native_terminal_commands::write_native_terminal,
            ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.invoke_handler(build_invoke_handler!());
    }

    builder.run(context).expect("error while running Workbench");
}
