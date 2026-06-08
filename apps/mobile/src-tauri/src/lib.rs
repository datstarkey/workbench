//! Workbench mobile — a thin Tauri shell whose webview hosts the shared
//! `@workbench/control-plane-ui` frontend pointed at a remote `workbench-server`
//! over HTTP. No PTY, no terminal IO, no embedded server: the phone is purely a
//! control-plane client (list/create worktrees, spawn/kill remote sessions).

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Forward Rust + (via attachConsole) frontend logs to the platform
        // logger — on Android that's logcat, so `adb logcat` shows app logs.
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .build(),
        )
        .run(tauri::generate_context!())
        .expect("error while running Workbench mobile");
}
