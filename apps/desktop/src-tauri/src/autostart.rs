//! Launch at login (macOS LaunchAgent, Windows HKCU Run key) over `auto-launch`.
//! Never part of setup's error path: a broken registration must not stop the app.

use tauri::{AppHandle, Env, Manager, Runtime};

/// Passed by every login launch, so the app starts minimised.
pub const ARG: &str = "--autostart";

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn entry_name(app: &AppHandle) -> String {
    // A dev build must never replace the installed app's login entry.
    if cfg!(debug_assertions) {
        format!("{} Dev", app.package_info().name)
    } else {
        app.package_info().name.clone()
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn current_exe() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    {
        let path = exe
            .canonicalize()
            .map_err(|e| e.to_string())?
            .display()
            .to_string();
        // A quarantined download runs from a temporary translocated path, and a
        // disk image is ejected: neither is there at the next login.
        if path.contains("/AppTranslocation/") || path.starts_with("/Volumes/") {
            return Err("Move Workbench to Applications first.".into());
        }
        Ok(path)
    }
    #[cfg(target_os = "windows")]
    Ok(exe.display().to_string())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn launcher(app: &AppHandle, exe: &str) -> Result<auto_launch::AutoLaunch, String> {
    // auto-launch writes the Run value as `{path} {args}` unquoted, so a path
    // with spaces would run `C:\Program.exe`.
    #[cfg(target_os = "windows")]
    let exe = &format!("\"{exe}\"");
    auto_launch::AutoLaunchBuilder::new()
        .set_app_name(&entry_name(app))
        .set_app_path(exe)
        .set_args(&[ARG])
        .set_use_launch_agent(true)
        .build()
        .map_err(|e| e.to_string())
}

/// A LaunchAgent plist's target and whether it passes `ARG`.
#[cfg(any(target_os = "macos", test))]
fn parse_plist(text: &str) -> Option<(String, bool)> {
    let array = text
        .split("<key>ProgramArguments</key>")
        .nth(1)?
        .split("</array>")
        .next()?;
    let mut args = array
        .split("<string>")
        .skip(1)
        .filter_map(|s| s.split("</string>").next());
    let target = args.next()?.to_string();
    Some((target, args.any(|a| a == ARG)))
}

/// A Run value's target and whether it passes `ARG`. Ours is
/// `"C:\…\Workbench.exe" --autostart`; the old plugin's an unquoted path alone.
#[cfg(any(target_os = "windows", test))]
fn parse_run_value(value: &str) -> Option<(String, bool)> {
    match value.trim().strip_prefix('"') {
        Some(quoted) => {
            let (target, rest) = quoted.split_once('"')?;
            Some((
                target.to_string(),
                rest.split_whitespace().any(|a| a == ARG),
            ))
        }
        None => Some((value.trim().to_string(), false)),
    }
}

#[cfg(target_os = "macos")]
fn registered(app: &AppHandle) -> Option<(String, bool)> {
    let plist = std::path::Path::new(&std::env::var_os("HOME")?)
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", entry_name(app)));
    parse_plist(&std::fs::read_to_string(plist).ok()?)
}

#[cfg(target_os = "windows")]
fn registered(app: &AppHandle) -> Option<(String, bool)> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let value: String = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run")
        .ok()?
        .get_value(entry_name(app))
        .ok()?;
    parse_run_value(&value)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[tauri::command]
pub fn autostart_enabled(app: AppHandle) -> Result<bool, String> {
    launcher(&app, &current_exe()?)?
        .is_enabled()
        .map_err(|e| e.to_string())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let launcher = launcher(&app, &current_exe()?)?;
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|e| e.to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const UNSUPPORTED: &str = "Start on startup isn't available on this platform.";

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[tauri::command]
pub fn autostart_enabled() -> Result<bool, String> {
    Err(UNSUPPORTED.into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[tauri::command]
pub fn set_autostart(_enabled: bool) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

/// An enabled entry from an older version gains `ARG`, and one whose target is
/// gone (the app moved) is pointed at this copy. One whose target still exists
/// keeps it, so another copy launched once doesn't take it over. Writes nothing
/// otherwise: macOS notifies about every LaunchAgent write.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn refresh(app: &AppHandle) -> Result<(), String> {
    let Some((target, has_arg)) = registered(app) else {
        return Ok(());
    };
    let exists = std::path::Path::new(&target).exists();
    if exists && has_arg {
        return Ok(());
    }
    let exe = if exists { target } else { current_exe()? };
    let launcher = launcher(app, &exe)?;
    // A Task Manager disable leaves the Run value in place; enabling would undo it.
    if !launcher.is_enabled().map_err(|e| e.to_string())? {
        return Ok(());
    }
    launcher.enable().map_err(|e| e.to_string())
}

/// Login launch: start minimised, then repair the login entry off the main thread.
pub fn on_startup(app: &AppHandle) {
    if std::env::args().any(|a| a == ARG) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.minimize();
        }
        forget_login_launch(app);
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if !cfg!(debug_assertions) {
        let app = app.clone();
        std::thread::spawn(move || {
            if let Err(e) = refresh(&app) {
                log::warn!("couldn't refresh the login item: {e}");
            }
        });
    }
}

/// A restart (`process::restart`) and the Windows updater's relaunch (NSIS
/// `/ARGS`, captured when the updater is built) reuse the managed `Env`'s args,
/// so a login launch would come back minimised after an update.
fn forget_login_launch<R: Runtime>(app: &AppHandle<R>) {
    // Nothing keeps a `State<Env>`: `Manager::env` clones it.
    #[allow(deprecated)]
    let Some(mut env) = app.unmanage::<Env>() else {
        return;
    };
    env.args_os.retain(|a| a != ARG);
    app.manage(env);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_launch_restarts_without_the_login_flag() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let handle = app.handle();
        #[allow(deprecated)]
        let mut env = handle.unmanage::<Env>().expect("env");
        env.args_os = vec!["workbench".into(), ARG.into(), "--x".into()];
        handle.manage(env);
        forget_login_launch(handle);
        assert_eq!(handle.env().args_os, vec!["workbench", "--x"]);
    }

    #[test]
    fn reads_a_plist_target_and_flag() {
        let ours = "<plist><dict><key>Label</key><string>Workbench</string>\
            <key>ProgramArguments</key>\n  <array><string>/Applications/Workbench.app/Contents/MacOS/workbench</string>\
            <string>--autostart</string></array><key>RunAtLoad</key><true/></dict></plist>";
        assert_eq!(
            parse_plist(ours),
            Some((
                "/Applications/Workbench.app/Contents/MacOS/workbench".into(),
                true
            ))
        );
        let old = "<key>ProgramArguments</key><array><string>/Applications/W.app/Contents/MacOS/w</string></array>";
        assert_eq!(
            parse_plist(old),
            Some(("/Applications/W.app/Contents/MacOS/w".into(), false))
        );
        assert_eq!(parse_plist("<plist></plist>"), None);
    }

    #[test]
    fn reads_a_run_value_quoted_or_from_the_old_plugin() {
        assert_eq!(
            parse_run_value(r#""C:\Program Files\Workbench\workbench.exe" --autostart"#),
            Some((r"C:\Program Files\Workbench\workbench.exe".into(), true))
        );
        assert_eq!(
            parse_run_value(r"C:\Users\Jake Starkey\AppData\Local\Workbench\workbench.exe "),
            Some((
                r"C:\Users\Jake Starkey\AppData\Local\Workbench\workbench.exe".into(),
                false
            ))
        );
        assert_eq!(parse_run_value(r#""C:\unterminated"#), None);
    }
}
