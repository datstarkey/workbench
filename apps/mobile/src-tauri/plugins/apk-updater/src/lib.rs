//! In-app updater for the sideloaded Android build: the Kotlin side downloads a
//! release APK from GitHub, verifies its SHA-256 and opens the system installer.
//! Everything lives in `android/`; this crate only registers it.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("apk-updater")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _api.register_android_plugin("com.workbench.apkupdater", "ApkUpdaterPlugin")?;
            Ok(())
        })
        .build()
}
