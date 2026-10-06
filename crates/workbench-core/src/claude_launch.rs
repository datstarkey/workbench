//! The `claude` command a server terminal runs for a known session. Built here
//! rather than by the client so a remote client can't launch Claude without the
//! sandbox wrapper or permission mode the desktop's settings require (mirrors
//! `claudeBinary` in `apps/desktop/src/lib/utils/claude.ts`).

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::claude_transcript::is_uuid;
use crate::types::WorkbenchSettings;

/// Modes the CLI accepts for `--permission-mode`.
pub const PERMISSION_MODES: &[&str] = &[
    "default",
    "acceptEdits",
    "plan",
    "auto",
    "dontAsk",
    "bypassPermissions",
];

/// Must match `SANDBOX_RUNTIME_PACKAGE` in `apps/desktop/src/lib/utils/claude.ts`;
/// see `docs/SANDBOX_RUNTIME.md` for why the npm version is pinned.
pub const SANDBOX_RUNTIME_PACKAGE: &str = "@anthropic-ai/sandbox-runtime@0.0.76";

/// `claude --resume <id>` (or `--session-id <id>` for a new session), carrying
/// the configured permission mode and, when the sandbox is on, srt's wrapper
/// pointing at `sandbox_settings`.
/// `resume_at`: continue from this entry, dropping what came after it (a rewind).
pub fn terminal_command(
    session_id: &str,
    resume: bool,
    resume_at: Option<&str>,
    settings: &WorkbenchSettings,
    sandbox_settings: &Path,
) -> Result<String> {
    if !is_uuid(session_id) {
        bail!("session id must be a UUID");
    }
    let mut cmd = String::new();
    if settings.sandbox_runtime_enabled && !cfg!(windows) {
        // Fail closed: launching unwrapped would silently drop the sandbox.
        if !sandbox_settings.is_file() {
            bail!(
                "The sandbox settings file is missing; open Workbench on the desktop to write it"
            );
        }
        let path = sandbox_settings
            .to_str()
            .context("the sandbox settings path is not UTF-8")?;
        // The `--` stops srt claiming claude's own flags.
        cmd.push_str(&format!(
            "npx --yes {SANDBOX_RUNTIME_PACKAGE} --settings {} -- ",
            shell_quote(path)
        ));
    }
    // Tests point it at a fake (`WORKBENCH_CLAUDE_BIN`, as for remote-control).
    match std::env::var("WORKBENCH_CLAUDE_BIN") {
        Ok(bin) if !bin.is_empty() => cmd.push_str(&shell_quote(&bin)),
        _ => cmd.push_str("claude"),
    }
    let mode = settings.claude_permission_mode.as_str();
    if mode != "default" && PERMISSION_MODES.contains(&mode) {
        cmd.push_str(&format!(" --permission-mode {mode}"));
    }
    let flag = if resume { "--resume" } else { "--session-id" };
    cmd.push_str(&format!(" {flag} {session_id}"));
    // An entry id from the session file; only a UUID reaches the shell, unquoted
    // (cmd.exe keeps single quotes in the argument).
    if let Some(at) = resume_at.filter(|at| resume && is_uuid(at)) {
        cmd.push_str(&format!(" --resume-session-at={at}"));
    }
    Ok(cmd)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

/// Keys that pick "Yes, I trust this folder" in Claude Code's trust dialog,
/// sent one at a time: it opens on "No, exit", so Enter alone quits.
pub const TRUST_ACCEPT_KEYS: [&[u8]; 2] = [b"\x1b[B", b"\r"];

/// Whether a terminal running `claude` shows its folder trust dialog. It comes
/// before any plugin loads, so a chat can't start until it's answered. The TUI
/// places words with cursor moves rather than spaces, so the text is compared
/// with escape sequences and whitespace removed.
pub fn shows_trust_prompt(output: &str) -> bool {
    let mut text = String::with_capacity(output.len());
    let mut chars = output.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.next() {
                // CSI: parameters, then one final byte in @..~.
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC: up to BEL or ST.
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if !c.is_whitespace() {
            text.push(c);
        }
    }
    text.contains("Yes,Itrustthisfolder")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SID: &str = "7b3c54f4-ba22-4654-9af9-037d1cd8e555";

    /// The dialog as Claude Code 2.1.291 drew it in a PTY.
    const TRUST_DIALOG: &str = "\x1b[2G\x1b[1mQuick\x1b[8Gsafety\x1b[15Gcheck\x1b[22m\r\r\n\x1b[2G\x1b[38;2;177;185;249m\u{276f}\x1b[4GNo,\x1b[8Gexit\x1b[39m\r\r\n\x1b[4GYes,\x1b[9GI\x1b[11Gtrust\x1b[17Gthis\x1b[22Gfolder\r\r\n\x1b]0;claude\x07";

    #[test]
    fn spots_the_trust_dialog_drawn_with_cursor_moves() {
        assert!(shows_trust_prompt(TRUST_DIALOG));
        assert!(shows_trust_prompt(&format!(
            "$ claude --resume x\r\n{TRUST_DIALOG}"
        )));
    }

    #[test]
    fn other_output_is_not_the_trust_dialog() {
        assert!(!shows_trust_prompt(
            "$ claude\r\n\x1b[1mWelcome to Claude Code\x1b[22m"
        ));
        assert!(!shows_trust_prompt("\x1b[4GNo,\x1b[8Gexit"));
        assert!(!shows_trust_prompt(""));
    }

    fn settings(mode: &str, sandbox: bool) -> WorkbenchSettings {
        WorkbenchSettings {
            claude_permission_mode: mode.into(),
            sandbox_runtime_enabled: sandbox,
            ..Default::default()
        }
    }

    #[test]
    fn default_mode_adds_no_flag() {
        let cmd = terminal_command(
            SID,
            true,
            None,
            &settings("default", false),
            Path::new("/x"),
        );
        assert_eq!(cmd.unwrap(), format!("claude --resume {SID}"));
    }

    #[test]
    fn known_modes_are_passed_and_unknown_ones_dropped() {
        let cmd = terminal_command(
            SID,
            false,
            None,
            &settings("acceptEdits", false),
            Path::new("/x"),
        );
        assert_eq!(
            cmd.unwrap(),
            format!("claude --permission-mode acceptEdits --session-id {SID}")
        );
        let cmd = terminal_command(
            SID,
            true,
            None,
            &settings("rm -rf /", false),
            Path::new("/x"),
        );
        assert_eq!(cmd.unwrap(), format!("claude --resume {SID}"));
    }

    #[test]
    fn rejects_a_non_uuid_id() {
        let bad = format!("{SID}; rm -rf ~");
        assert!(terminal_command(
            &bad,
            true,
            None,
            &settings("default", false),
            Path::new("/x")
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn sandbox_wraps_with_a_quoted_settings_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("it's.json");
        std::fs::write(&file, "{}").unwrap();
        let cmd = terminal_command(SID, true, None, &settings("plan", true), &file).unwrap();
        let quoted = file.to_str().unwrap().replace('\'', r#"'"'"'"#);
        assert_eq!(
            cmd,
            format!(
                "npx --yes {SANDBOX_RUNTIME_PACKAGE} --settings '{quoted}' -- claude --permission-mode plan --resume {SID}"
            )
        );
    }

    #[test]
    fn a_rewind_resumes_at_its_fork_point() {
        let at = "5e5e5e5e-0000-4000-8000-000000000001";
        let cmd = terminal_command(
            SID,
            true,
            Some(at),
            &settings("default", false),
            Path::new("/x"),
        )
        .unwrap();
        assert!(cmd.ends_with(&format!("--resume {SID} --resume-session-at={at}")));
        let cmd = terminal_command(
            SID,
            true,
            Some("x; rm -rf /"),
            &settings("default", false),
            Path::new("/x"),
        )
        .unwrap();
        assert!(
            !cmd.contains("resume-session-at"),
            "only a UUID reaches the shell: {cmd}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn sandbox_without_its_settings_file_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let err = terminal_command(
            SID,
            true,
            None,
            &settings("default", true),
            &dir.path().join("no.json"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("sandbox settings file is missing"));
    }
}
