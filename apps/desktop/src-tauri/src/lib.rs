// Pure logic now lives in `workbench-core`. Re-export each moved module at the
// crate root so existing `crate::config`, `crate::git`, `crate::types`, … paths
// throughout the desktop crate keep resolving without per-file edits.
pub use workbench_core::{
    claude_accounts, claude_sessions, codex_config, codex_sessions, config, git, github, net,
    package_scripts, paths, sandbox_runtime, session_utils, settings, shell, shell_integration,
    text, trello, trello_automation, types, workspace, worktrees,
};

// The e2e WebDriver server is unauthenticated control of the webview (and so of
// every Tauri command) for anything on loopback, including sandboxed sessions.
#[cfg(all(feature = "e2e", not(debug_assertions)))]
compile_error!("the `e2e` feature is test-only and must not be built in release mode");

mod autostart;
mod commands;
mod git_commands;
mod git_watcher;
mod github_poller;
mod hook_bridge;
mod host_update;
// Windows and Linux menu bars can't follow the dark theme (Win32 draws a light
// one regardless), and their title bars already have the window controls.
#[cfg(target_os = "macos")]
mod menu;
#[cfg(target_os = "macos")]
mod native_notification_commands;
#[cfg(target_os = "macos")]
mod native_notifications;
#[cfg(target_os = "macos")]
mod native_terminal;
#[cfg(target_os = "macos")]
mod native_terminal_commands;
mod observability;
mod refresh_dispatcher;
mod server_control;
mod trello_commands;

use git_watcher::GitWatcher;
use github_poller::GitHubPoller;
use hook_bridge::HookBridgeState;
use refresh_dispatcher::RefreshDispatcher;
use tauri::Manager;

/// Run a command's blocking work (git, gh, file reads) on Tauri's blocking
/// pool. A `command(async)` sync fn runs on the runtime's workers instead, so a
/// burst of them held up every other command and event until they finished.
pub(crate) async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    // tokio's handle (commands run on Tauri's tokio runtime) keeps the panic:
    // it is re-raised as itself, not reported twice under a vaguer message.
    match tokio::task::spawn_blocking(work).await {
        Ok(value) => value,
        Err(e) => match e.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(e) => panic!("a blocking command was cancelled: {e}"),
        },
    }
}

/// Build the invoke handler with all shared commands, plus native terminal
/// commands on macOS. Uses a declarative macro to avoid duplicating the
/// shared command list across cfg branches.
macro_rules! build_invoke_handler {
    ( $( $extra:path ),* $(,)? ) => {
        tauri::generate_handler![
            autostart::autostart_enabled,
            autostart::set_autostart,
            commands::list_projects,
            commands::read_chat_attachment,
            commands::save_projects,
            commands::open_in_vscode,
            commands::watch_git_projects,
            commands::discover_claude_sessions,
            commands::claude_auth_status,
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
            commands::sandbox_runtime_settings_path,
            commands::github_is_available,
            commands::github_get_remote,
            commands::github_set_tracked_projects,
            commands::github_refresh_project,
            commands::github_update_pr_branch,
            commands::github_rerun_workflow,
            commands::github_mark_pr_ready,
            commands::github_merge_pr,
            commands::github_list_repos,
            commands::github_checkout_pr,
            commands::github_fetch_pr_branch,
            commands::clone_repo,
            commands::delete_branch,
            commands::open_url,
            commands::check_codex_integration,
            commands::codex_supports_no_daemon,
            commands::apply_codex_integration,
            commands::get_hook_logs,
            commands::clear_hook_logs,
            commands::is_native_terminal_available,
            commands::get_package_info,
            git_commands::git_status,
            git_commands::git_file_diff,
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
            server_control::rotate_server_token,
            server_control::set_active_claude_account,
            server_control::pairing_addresses,
            host_update::host_update_status,
            host_update::host_update_install,
            $( $extra ),*
        ]
    };
}

/// `tauri dev` is its own instance (`~/.workbench-dev`), so it never rewrites
/// the installed app's projects, settings or Claude plugin copy; a `--debug`
/// build stays on `~/.workbench`. The first run copies the project list over.
fn use_dev_config_dir() {
    if std::env::var_os("WORKBENCH_CONFIG_DIR").is_some() {
        return;
    }
    let dir = paths::home_dir().join(".workbench-dev");
    let projects = dir.join("projects.json");
    if !projects.exists() && std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::copy(
            paths::home_dir().join(".workbench/projects.json"),
            &projects,
        );
    }
    paths::set_workbench_config_dir(dir);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // First, before any thread starts: a Workbench launched from another's
    // terminal must not report to that one's server, pane or plugin copy.
    workbench_core::shell::scrub_inherited_env();
    if tauri::is_dev() {
        use_dev_config_dir();
    }
    let context = tauri::generate_context!();

    // Initialise backend error reporting before building the app so panics in
    // any thread are captured. No-op in debug builds. Guard kept alive for the
    // whole process — dropping it flushes pending events on shutdown.
    let _sentry_guard = observability::init(context.package_info().version.to_string());
    workbench_server::watchdog::raise_fd_limit();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .manage(RefreshDispatcher::new())
        .manage(server_control::ServerControl::new())
        .manage(host_update::UpdateGuard::default())
        .setup(|app| {
            autostart::on_startup(app.handle());

            let handle = app.handle().clone();
            #[cfg(target_os = "macos")]
            menu::build(&handle).expect("failed to build menu");
            // The server owns the hook bridge (and names it to the sandbox file).
            let hooks = app.state::<server_control::ServerControl>().hooks();
            let bridge = HookBridgeState::new(handle.clone(), hooks);
            app.manage(bridge);
            // Activity now comes from the `workbench` Claude Code plugin; drop the
            // hook script older versions registered so events aren't reported twice.
            std::thread::spawn(|| {
                if let Err(e) = settings::remove_workbench_hook_integration() {
                    log::warn!("failed to remove the old Claude hook script: {e}");
                }
            });
            let git_watcher = GitWatcher::new(handle);
            app.manage(git_watcher);
            let github_poller = GitHubPoller::new(app.handle().clone());
            app.manage(github_poller);

            // Before the app finishes launching: macOS drops click responses for any
            // notification delivered before the delegate is installed.
            #[cfg(target_os = "macos")]
            native_notifications::init(app.handle().clone());

            // Boot the always-on loopback server (127.0.0.1, ephemeral port)
            // synchronously on the Tauri async runtime so it is listening
            // before the webview mounts. `spawn_embedded` binds before
            // returning, so by the time setup() returns the server is ready.
            let host = std::sync::Arc::new(host_update::DesktopHost::new(app.handle().clone()));
            app.manage(host.clone());
            let sc = app.state::<server_control::ServerControl>();
            sc.set_host(host);
            // Degrade instead of aborting launch: terminals will fail to connect
            // (surfaced per-pane) but the rest of the app still works. A hard
            // panic here would take down the whole window on a transient bind
            // failure (port exhaustion, sandbox), which terminals never used to
            // require.
            if let Err(e) = tauri::async_runtime::block_on(sc.start_loopback()) {
                log::error!("failed to start loopback embedded server: {e}");
            }
            sc.watch_attention(app.handle().clone());
            sc.watch_settings(app.handle().clone());

            Ok(())
        });

    #[cfg(target_os = "macos")]
    {
        builder = builder
            .manage(native_terminal::NativeTerminalManager::new())
            .invoke_handler(build_invoke_handler!(
                native_terminal_commands::attach_native_terminal,
                native_terminal_commands::resize_native_terminal,
                native_terminal_commands::set_native_terminal_visible,
                native_terminal_commands::detach_native_terminal,
                native_terminal_commands::write_native_terminal,
                native_notification_commands::is_native_notification_available,
                native_notification_commands::send_native_notification,
                native_notification_commands::remove_native_notification,
            ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.invoke_handler(build_invoke_handler!());
    }

    #[cfg(feature = "e2e")]
    {
        builder = builder.plugin(tauri_plugin_wdio_webdriver::init());
    }

    builder.run(context).expect("error while running Workbench");
}
