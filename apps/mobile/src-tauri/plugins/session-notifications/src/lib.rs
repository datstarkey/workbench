use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("session-notifications")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _api.register_android_plugin(
                "com.workbench.notifications",
                "SessionNotificationsPlugin",
            )?;
            Ok(())
        })
        .build()
}
