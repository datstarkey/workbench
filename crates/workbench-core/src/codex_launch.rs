//! The `codex` command a terminal runs for a Codex TUI pane, built from the
//! host's saved settings as `claude_launch` builds Claude's, so no client
//! sends a raw command for an AI pane.

use std::path::Path;

use anyhow::{bail, Result};
use chrono::{Duration, Local, TimeZone};
use serde::Deserialize;

use crate::claude_transcript::is_uuid;
use crate::codex_controls::{APPROVAL_POLICIES, SANDBOX_MODES};
use crate::launch_prompt;
use crate::types::WorkbenchSettings;

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

/// Whether Codex has a thread on disk to `resume`. Codex writes a thread to
/// `sessions/YYYY/MM/DD/rollout-<time>-<id>.jsonl`, dated when it started,
/// which a UUIDv7 id carries: only that day's folder (and its neighbours, for
/// time zones) is read. Any other id falls back to walking every folder.
pub fn thread_exists(id: &str) -> bool {
    thread_exists_in(&crate::paths::codex_sessions_dir(), id)
}

fn thread_exists_in(sessions: &Path, id: &str) -> bool {
    let suffix = format!("-{id}.jsonl");
    let named = |p: &Path| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(&suffix))
    };
    let Some(started) = uuid_v7_ms(id).and_then(|ms| Local.timestamp_millis_opt(ms).single())
    else {
        return crate::codex_sessions::collect_jsonl_files(sessions, 4)
            .iter()
            .any(|f| named(f));
    };
    [-1, 0, 1].into_iter().any(|days| {
        let day = (started + Duration::days(days))
            .format("%Y/%m/%d")
            .to_string();
        std::fs::read_dir(sessions.join(day))
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| named(&e.path()))
    })
}

/// The Unix milliseconds a UUIDv7 starts with.
fn uuid_v7_ms(id: &str) -> Option<i64> {
    let hex: String = id.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 || hex.as_bytes()[12] != b'7' {
        return None;
    }
    i64::from_str_radix(&hex[..12], 16).ok()
}

/// `codex [--no-daemon] -c tui.alternate_screen=never [-c overrides] [resume <id> | -- <prompt>]`.
/// `no_daemon` is whether the installed CLI takes `--no-daemon`. A prompt
/// Windows' shells can't take is left out (see [`prompt_notice`]).
pub fn terminal_command(
    session: &CodexSessionLaunch,
    settings: &WorkbenchSettings,
    no_daemon: bool,
) -> Result<String> {
    // Tests point it at a fake (`WORKBENCH_CODEX_BIN`).
    let mut cmd = match std::env::var("WORKBENCH_CODEX_BIN") {
        Ok(bin) if !bin.is_empty() => launch_prompt::shell_quote(&bin),
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
            // `--` ends codex's options (clap), so a prompt like
            // `--dangerously-bypass-approvals-and-sandbox` is the prompt, never a flag.
            if let Some(arg) = new_prompt(session).as_deref().and_then(launch_prompt::arg) {
                cmd.push_str(" -- ");
                cmd.push_str(&arg);
            }
        }
    }
    Ok(cmd)
}

/// Why a new thread's prompt was left out of its command, to tell the person.
pub fn prompt_notice(session: &CodexSessionLaunch) -> Option<String> {
    let prompt = new_prompt(session)?;
    launch_prompt::arg(&prompt)
        .is_none()
        .then(|| launch_prompt::dropped_notice("Codex"))
}

/// A resumed thread already started with its prompt.
fn new_prompt(session: &CodexSessionLaunch) -> Option<String> {
    launch_prompt::normalize(session.prompt.as_deref().filter(|_| session.id.is_none()))
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
        let bad = CodexSessionLaunch {
            id: Some("--x".into()),
            prompt: None,
        };
        assert!(terminal_command(&bad, &s, false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_prompt_is_never_read_as_a_flag() {
        let s = settings("default", "default");
        let launch = |prompt: &str| CodexSessionLaunch {
            id: None,
            prompt: Some(prompt.into()),
        };
        let cmd = terminal_command(&launch(" it's\r\n"), &s, false).unwrap();
        assert!(cmd.ends_with(r#" -- 'it'"'"'s'"#), "{cmd}");
        let cmd = terminal_command(
            &launch("--dangerously-bypass-approvals-and-sandbox"),
            &s,
            false,
        )
        .unwrap();
        assert!(
            cmd.ends_with(" -- '--dangerously-bypass-approvals-and-sandbox'"),
            "{cmd}"
        );
        assert_eq!(prompt_notice(&launch("fine")), None);
    }

    #[test]
    fn finds_a_thread_in_its_start_days_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let id = "019dd4f4-8f05-7e10-9d3d-2f4ab5c4f0a1";
        assert!(!thread_exists_in(tmp.path(), id));
        let ms = uuid_v7_ms(id).unwrap();
        let day = Local
            .timestamp_millis_opt(ms)
            .unwrap()
            .format("%Y/%m/%d")
            .to_string();
        let dir = tmp.path().join(day);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("rollout-2026-01-01T00-00-00-{id}.jsonl")),
            "",
        )
        .unwrap();
        assert!(thread_exists_in(tmp.path(), id));
        // A v4 id walks every folder.
        let v4_dir = tmp.path().join("2025/01/01");
        std::fs::create_dir_all(&v4_dir).unwrap();
        std::fs::write(v4_dir.join(format!("rollout-x-{TID}.jsonl")), "").unwrap();
        assert!(thread_exists_in(tmp.path(), TID));
    }
}
