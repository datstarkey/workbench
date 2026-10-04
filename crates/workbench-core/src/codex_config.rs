/// Codex CLI configuration management.
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;
use toml_edit::{value, Array, Document};

use crate::paths;

#[cfg(not(windows))]
const WORKBENCH_CODEX_NOTIFY_SCRIPT_NAME: &str = "workbench-codex-notify-bridge.sh";
#[cfg(windows)]
const WORKBENCH_CODEX_NOTIFY_SCRIPT_NAME: &str = "workbench-codex-notify-bridge.ps1";

/// `WORKBENCH_CODEX_BIN`, else `codex` found on the enriched search path
/// (npm installs a `codex.cmd` shim on Windows).
pub fn codex_binary() -> PathBuf {
    let names: &[&str] = if cfg!(windows) {
        &["codex.exe", "codex.cmd"]
    } else {
        &["codex"]
    };
    paths::find_binary("WORKBENCH_CODEX_BIN", names)
}

/// The installed CLI decides whether a Workbench terminal can opt out of its
/// shared daemon. Cache per binary; bounded help probe never reads credentials.
pub fn supports_no_daemon() -> bool {
    use std::{
        collections::HashMap,
        io::Read,
        process::Stdio,
        sync::{Mutex, OnceLock},
        time::{Duration, Instant},
    };
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, bool>>> = OnceLock::new();
    let binary = codex_binary();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&binary) {
        return *v;
    }
    let supported = (|| {
        let mut child = crate::shell::command(&binary)
            .arg("--help")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let stdout = child.stdout.take()?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut out = String::new();
            let result = stdout.take(64 * 1024).read_to_string(&mut out).map(|_| out);
            let _ = tx.send(result);
        });
        loop {
            if child.try_wait().ok()?.is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Some(false);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let out = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok()?
            .ok()?;
        Some(out.contains("--no-daemon"))
    })()
    .unwrap_or(false);
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(binary, supported);
    supported
}

fn workbench_codex_notify_script_path() -> PathBuf {
    paths::codex_config_dir().join(WORKBENCH_CODEX_NOTIFY_SCRIPT_NAME)
}

#[cfg(not(windows))]
fn workbench_codex_notify_script_body() -> &'static str {
    "#!/usr/bin/env bash\n\
SOCKET=\"${WORKBENCH_HOOK_SOCKET}\"\n\
PANE_ID=\"${WORKBENCH_PANE_ID}\"\n\
[[ -z \"$SOCKET\" || -z \"$PANE_ID\" || -z \"$1\" ]] && exit 0\n\
PAYLOAD=$(printf '%s' \"$1\" | tr -d '\\n\\r')\n\
IFS=: read -r HOST PORT <<< \"$SOCKET\"\n\
exec 3<>/dev/tcp/\"$HOST\"/\"$PORT\" 2>/dev/null || exit 0\n\
printf '{\"pane_id\":\"%s\",\"codex\":%s}\\n' \"$PANE_ID\" \"$PAYLOAD\" >&3\n"
}

#[cfg(windows)]
fn workbench_codex_notify_script_body() -> &'static str {
    "$socket = $env:WORKBENCH_HOOK_SOCKET\n\
$paneId = $env:WORKBENCH_PANE_ID\n\
if (-not $socket -or -not $paneId -or $args.Count -eq 0) { exit 0 }\n\
$payload = ($args[0] -replace '\\s+', ' ').Trim()\n\
$msg = [Text.Encoding]::UTF8.GetBytes(\"{`\"pane_id`\":`\"$paneId`\",`\"codex`\":$payload}`n\")\n\
try {\n\
    $parts = $socket -split ':'\n\
    $tcp = [Net.Sockets.TcpClient]::new($parts[0], [int]$parts[1])\n\
    $tcp.GetStream().Write($msg, 0, $msg.Length)\n\
    $tcp.Close()\n\
} catch { }\n"
}

/// The previous integration is retained in a separate file so regenerating the
/// Workbench bridge does not discard it. Arguments are quoted as data, never
/// evaluated as shell source, and receive Codex's original payload unchanged.
fn chained_script(previous: &[String]) -> String {
    let mut body = workbench_codex_notify_script_body().to_string();
    if previous.is_empty() {
        return body;
    }
    #[cfg(not(windows))]
    {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let command = previous
            .iter()
            .map(|s| quote(s))
            .collect::<Vec<_>>()
            .join(" ");
        body = body.replacen(
            "#!/usr/bin/env bash\n",
            &format!("#!/usr/bin/env bash\n{command} \"$@\" || true\n"),
            1,
        );
    }
    #[cfg(windows)]
    {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let command = previous
            .iter()
            .map(|s| quote(s))
            .collect::<Vec<_>>()
            .join(" ");
        body = format!("try {{ & {command} @args }} catch {{ }}\n{body}");
    }
    body
}

fn notify_args(script_path: &str) -> Vec<String> {
    #[cfg(not(windows))]
    let args = vec!["bash", script_path];
    #[cfg(windows)]
    let args = vec![
        "powershell.exe",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        script_path,
    ];
    args.into_iter().map(String::from).collect()
}

fn string_array(item: Option<&toml_edit::Item>) -> Option<Vec<String>> {
    item?
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(String::from))
        .collect()
}

struct ConfigEdit {
    text: String,
    installed: bool,
    previous: Option<Vec<String>>,
}

/// Parse before writing either config or scripts. Invalid/unsupported notify
/// values are errors instead of silently destroying an existing integration.
fn edit_config(content: &str, script_path: &str) -> Result<ConfigEdit> {
    let mut doc = content
        .parse::<Document>()
        .context("invalid Codex config.toml")?;
    let expected = notify_args(script_path);
    let old = string_array(doc.get("notify"));
    if doc.get("notify").is_some() && old.is_none() {
        bail!("Codex notify must be an array of strings; existing integration was preserved");
    }
    let installed = old.as_ref() == Some(&expected);
    let previous = old.filter(|old| old != &expected);
    if previous
        .as_ref()
        .is_some_and(|args| args.iter().any(|arg| arg == script_path))
    {
        bail!("Existing notify already references the Workbench bridge with different arguments; refusing a recursive hook");
    }
    let mut notify = Array::new();
    for arg in expected {
        notify.push(arg);
    }
    if string_array(doc.get("notify")) != string_array(Some(&value(notify.clone()))) {
        let mut replacement = value(notify);
        if let Some(decor) = doc
            .get("notify")
            .and_then(toml_edit::Item::as_value)
            .map(|v| v.decor().clone())
        {
            *replacement.as_value_mut().unwrap().decor_mut() = decor;
        }
        doc["notify"] = replacement;
    }
    if doc.get("project_doc_fallback_filenames").is_some()
        && string_array(doc.get("project_doc_fallback_filenames")).is_none()
    {
        bail!("Codex project_doc_fallback_filenames must be an array of strings");
    }
    let mut fallback = doc
        .get("project_doc_fallback_filenames")
        .and_then(toml_edit::Item::as_array)
        .cloned()
        .unwrap_or_default();
    if !fallback.iter().any(|v| v.as_str() == Some("CLAUDE.md")) {
        fallback.push("CLAUDE.md");
        doc["project_doc_fallback_filenames"] = value(fallback);
    }
    Ok(ConfigEdit {
        text: doc.to_string(),
        installed,
        previous,
    })
}

pub fn check_codex_config_status() -> crate::types::IntegrationStatus {
    let dir = paths::codex_config_dir();
    let script = workbench_codex_notify_script_path();
    let installed = fs::read_to_string(dir.join("config.toml"))
        .ok()
        .and_then(|s| s.parse::<Document>().ok())
        .is_some_and(|doc| {
            string_array(doc.get("notify")) == Some(notify_args(&script.to_string_lossy()))
                && string_array(doc.get("project_doc_fallback_filenames"))
                    .is_some_and(|v| v.iter().any(|s| s == "CLAUDE.md"))
                && script.exists()
        });
    crate::types::IntegrationStatus {
        needs_changes: !installed,
        description: if installed {
            String::new()
        } else {
            format!("Workbench will update {} to add CLAUDE.md as a project doc fallback and install a notify bridge, preserving your existing notify command.", dir.join("config.toml").display())
        },
    }
}

pub fn ensure_codex_config() -> Result<()> {
    let dir = paths::codex_config_dir();
    let config = dir.join("config.toml");
    let content = if config.exists() {
        fs::read_to_string(&config)?
    } else {
        String::new()
    };
    let script = workbench_codex_notify_script_path();
    let edit = edit_config(&content, &script.to_string_lossy())?;
    let backup = dir.join("workbench-notify-previous.json");
    let replaced = !edit.installed;
    let previous = match edit.previous {
        Some(args) => args,
        None if edit.installed && backup.exists() => {
            serde_json::from_slice::<Vec<String>>(&fs::read(&backup)?)
                .context("invalid saved Codex notify command")?
        }
        None => Vec::new(),
    };
    fs::create_dir_all(&dir)?;
    // Save the chain before changing config so a retry retains the original.
    if replaced || !previous.is_empty() {
        paths::atomic_write(&backup, &serde_json::to_string(&previous)?)?;
    }
    paths::ensure_script(&script, &chained_script(&previous))?;
    if edit.text != content {
        paths::atomic_write(&config, &edit.text)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_keys_are_inserted_before_tables_and_comments_do_not_count() {
        let content = "# notify = [\"/bridge\"]\n[model_providers.example]\nname = \"Example\"\n";
        let edit = edit_config(content, "/bridge").unwrap();
        let doc = edit.text.parse::<Document>().unwrap();
        assert_eq!(
            string_array(doc.get("notify")),
            Some(notify_args("/bridge"))
        );
        assert_eq!(
            string_array(doc.get("project_doc_fallback_filenames")),
            Some(vec!["CLAUDE.md".into()])
        );
        assert!(doc["model_providers"]["example"].get("notify").is_none());
        assert!(edit.text.contains("# notify ="));
        assert_eq!(edit_config(&edit.text, "/bridge").unwrap().text, edit.text);
    }

    #[test]
    fn preserves_existing_fallbacks_and_notify_arguments() {
        let content = "notify=[\"python\", \"my hook.py\"] # user's hook\nproject_doc_fallback_filenames = [\"INSTRUCTIONS.md\"]\n[custom]\nnotify = [\"unrelated\"]\n";
        let edit = edit_config(content, "/bridge").unwrap();
        assert_eq!(
            edit.previous,
            Some(vec!["python".into(), "my hook.py".into()])
        );
        let doc = edit.text.parse::<Document>().unwrap();
        assert_eq!(
            string_array(doc.get("project_doc_fallback_filenames")),
            Some(vec!["INSTRUCTIONS.md".into(), "CLAUDE.md".into()])
        );
        assert_eq!(doc["custom"]["notify"][0].as_str(), Some("unrelated"));
        assert!(edit_config(&edit.text, "/bridge")
            .unwrap()
            .previous
            .is_none());
    }

    #[test]
    fn malformed_config_is_not_rewritten() {
        assert!(edit_config("[broken", "/bridge").is_err());
        assert!(edit_config("notify = 42", "/bridge").is_err());
        assert!(edit_config("project_doc_fallback_filenames = false", "/bridge").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn previous_notify_receives_the_payload_without_shell_evaluation() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let previous = dir.path().join("old hook.sh");
        let output = dir.path().join("payload.txt");
        fs::write(&previous, "#!/bin/sh\nprintf '%s' \"$2\" > \"$1\"\n").unwrap();
        fs::set_permissions(&previous, fs::Permissions::from_mode(0o755)).unwrap();
        let script = dir.path().join("bridge.sh");
        fs::write(
            &script,
            chained_script(&[
                previous.to_string_lossy().into(),
                output.to_string_lossy().into(),
            ]),
        )
        .unwrap();
        let payload = "{\"text\":\"`touch BAD` $(touch BAD) ' quote\"}";
        let status = crate::shell::command("bash")
            .arg(script)
            .arg(payload)
            .env_remove("WORKBENCH_HOOK_SOCKET")
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(fs::read_to_string(output).unwrap(), payload);
        assert!(!dir.path().join("BAD").exists());
    }
}
