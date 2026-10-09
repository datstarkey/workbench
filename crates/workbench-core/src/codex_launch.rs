//! The `codex` command a terminal runs for a Codex TUI pane, built from the
//! host's saved settings as `claude_launch` builds Claude's, so no client
//! sends a raw command for an AI pane.

use anyhow::{bail, Result};
use serde::Deserialize;

use crate::claude_transcript::is_uuid;
use crate::types::WorkbenchSettings;

const APPROVAL_POLICIES: &[&str] = &["never", "on-request", "untrusted", "on-failure"];
const SANDBOX_MODES: &[&str] = &["read-only", "workspace-write", "danger-full-access"];

/// The Codex thread a terminal runs (`codexSession` in a terminal create).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionLaunch {
    /// The thread to resume; absent, or a thread with nothing on disk, starts a new one.
    #[serde(default)]
    pub id: Option<String>,
    /// A new thread's first prompt.
    #[serde(default)]
    pub prompt: Option<String>,
}

/// Whether Codex has a thread on disk to `resume`.
pub fn thread_exists(id: &str) -> bool {
    let suffix = format!("{id}.jsonl");
    crate::codex_sessions::collect_jsonl_files(&crate::paths::codex_sessions_dir(), 4)
        .iter()
        .any(|f| {
            f.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(&suffix))
        })
}

/// `codex [--no-daemon] -c tui.alternate_screen=never [-c overrides] [resume <id> | <prompt>]`.
/// `no_daemon` is whether the installed CLI takes `--no-daemon`.
pub fn terminal_command(
    session: &CodexSessionLaunch,
    settings: &WorkbenchSettings,
    no_daemon: bool,
) -> Result<String> {
    // Tests point it at a fake (`WORKBENCH_CODEX_BIN`).
    let mut cmd = match std::env::var("WORKBENCH_CODEX_BIN") {
        Ok(bin) if !bin.is_empty() => shell_quote(&bin),
        _ => "codex".to_string(),
    };
    if no_daemon {
        cmd.push_str(" --no-daemon");
    }
    // Inline rather than the alternate screen, which has no scrollback. `-c`
    // keys, not flags: older builds ignore unknown keys but refuse unknown flags.
    cmd.push_str(" -c tui.alternate_screen=never");
    for (key, value, allowed) in [
        (
            "approval_policy",
            &settings.codex_approval_policy,
            APPROVAL_POLICIES,
        ),
        ("sandbox_mode", &settings.codex_sandbox_mode, SANDBOX_MODES),
    ] {
        if allowed.contains(&value.as_str()) {
            cmd.push_str(&format!(" -c {key}={value}"));
        }
    }
    match session.id.as_deref() {
        Some(id) if !is_uuid(id) => bail!("Codex thread id must be a UUID"),
        Some(id) => cmd.push_str(&format!(" resume {id}")),
        None => {
            if let Some(prompt) = new_prompt(session.prompt.as_deref()) {
                cmd.push(' ');
                cmd.push_str(&prompt_arg(&prompt)?);
            }
        }
    }
    Ok(cmd)
}

fn new_prompt(prompt: Option<&str>) -> Option<String> {
    let prompt = prompt?.replace("\r\n", "\n").replace('\r', "\n");
    let prompt = prompt.trim();
    (!prompt.is_empty()).then(|| prompt.to_string())
}

fn prompt_arg(prompt: &str) -> Result<String> {
    if cfg!(windows) {
        if prompt.contains(['"', '%', '$', '`', '!', '\n']) {
            bail!("On Windows a Codex prompt can't contain \" % $ ` ! or line breaks");
        }
        return Ok(format!("\"{prompt}\""));
    }
    Ok(shell_quote(prompt))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TID: &str = "7b3c54f4-ba22-4654-9af9-037d1cd8e555";

    fn settings(approval: &str, sandbox: &str) -> WorkbenchSettings {
        WorkbenchSettings {
            codex_approval_policy: approval.into(),
            codex_sandbox_mode: sandbox.into(),
            ..Default::default()
        }
    }

    #[test]
    fn carries_the_saved_overrides_and_skips_default() {
        let cmd =
            terminal_command(&Default::default(), &settings("never", "default"), true).unwrap();
        assert!(
            cmd.ends_with(" --no-daemon -c tui.alternate_screen=never -c approval_policy=never"),
            "{cmd}"
        );
        let cmd =
            terminal_command(&Default::default(), &settings("x; rm", "read-only"), false).unwrap();
        assert!(
            cmd.ends_with("-c tui.alternate_screen=never -c sandbox_mode=read-only"),
            "{cmd}"
        );
    }

    #[test]
    fn resumes_a_thread_or_starts_with_a_prompt() {
        let s = settings("default", "default");
        let resume = CodexSessionLaunch {
            id: Some(TID.into()),
            prompt: Some("hi".into()),
        };
        assert!(terminal_command(&resume, &s, false)
            .unwrap()
            .ends_with(&format!(" resume {TID}")));
        let fresh = CodexSessionLaunch {
            id: None,
            prompt: Some(" it's\r\n".into()),
        };
        #[cfg(unix)]
        assert!(terminal_command(&fresh, &s, false)
            .unwrap()
            .ends_with(r#" 'it'"'"'s'"#));
        let bad = CodexSessionLaunch {
            id: Some("--x".into()),
            prompt: None,
        };
        assert!(terminal_command(&bad, &s, false).is_err());
    }
}
