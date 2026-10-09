//! The `claude` command a terminal runs for a known session. Built here rather
//! than by the client so no client (a phone, or the desktop's own webview) can
//! launch Claude without the sandbox wrapper or permission mode the desktop's
//! settings require. Server terminals and the desktop's native macOS terminals
//! both build it here.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::claude_transcript::is_uuid;
use crate::launch_prompt;
use crate::types::{ProjectConfig, WorkbenchSettings};

/// Modes the CLI accepts for `--permission-mode`.
pub const PERMISSION_MODES: &[&str] = &[
    "default",
    "acceptEdits",
    "plan",
    "auto",
    "dontAsk",
    "bypassPermissions",
];

/// See `docs/SANDBOX_RUNTIME.md` for why the npm version is pinned.
pub const SANDBOX_RUNTIME_PACKAGE: &str = "@anthropic-ai/sandbox-runtime@0.0.76";

/// The Claude session a terminal runs (`claudeSession` in a terminal create).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ClaudeSessionLaunch {
    pub id: String,
    /// `--resume` an existing conversation, else `--session-id` starts one.
    /// A terminal create decides it from the session's history, whatever a
    /// client sent.
    #[serde(default)]
    pub resume: bool,
    /// With `resume`: continue from this entry, dropping what came after (a rewind).
    #[serde(default)]
    pub resume_at: Option<String>,
    /// A mode picked in chat, over the configured one (a restart to switch modes).
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// A new session's first prompt, submitted as Claude starts (an agent action).
    #[serde(default)]
    pub prompt: Option<String>,
}

/// What a new terminal types once its shell starts: `command`, or Claude on
/// `session` under the host's saved settings and projects (never both).
pub fn startup_command(
    command: Option<String>,
    session: Option<&ClaudeSessionLaunch>,
    settings: &WorkbenchSettings,
    projects: &[ProjectConfig],
) -> Result<Option<String>> {
    match (command, session) {
        (Some(_), Some(_)) => bail!("send either command or claudeSession, not both"),
        (_, Some(session)) => {
            let sandbox = if sandboxed(settings) {
                crate::sandbox_runtime::refresh_with(settings, projects)
                    .context("couldn't write the sandbox settings file")?
            } else {
                crate::sandbox_runtime::settings_path()
            };
            terminal_command(session, settings, &sandbox).map(Some)
        }
        (command, None) => Ok(command),
    }
}

/// srt wraps Claude when enabled; its Windows support is alpha, so never there.
fn sandboxed(settings: &WorkbenchSettings) -> bool {
    settings.sandbox_runtime_enabled && !cfg!(windows)
}

/// The `--permission-mode` a Claude terminal starts with: a `picked` one, else
/// the configured one unless that's `default` (Claude's settings decide).
pub fn launch_mode<'a>(
    picked: Option<&'a str>,
    settings: &'a WorkbenchSettings,
) -> Option<&'a str> {
    let mode = picked.unwrap_or(settings.claude_permission_mode.as_str());
    ((picked.is_some() || mode != "default") && PERMISSION_MODES.contains(&mode)).then_some(mode)
}

/// `claude --resume <id>` (or `--session-id <id>` for a new session), carrying
/// the configured permission mode and, when the sandbox is on, srt's wrapper
/// pointing at `sandbox_settings`. A picked `permission_mode` is passed even
/// when it's `default` (the configured `default` leaves the flag out, so
/// Claude's settings decide).
pub fn terminal_command(
    session: &ClaudeSessionLaunch,
    settings: &WorkbenchSettings,
    sandbox_settings: &Path,
) -> Result<String> {
    if !is_uuid(&session.id) {
        bail!("session id must be a UUID");
    }
    let mut cmd = String::new();
    if sandboxed(settings) {
        // Fail closed: launching unwrapped would silently drop the sandbox.
        if !sandbox_settings.is_file() {
            bail!("The sandbox settings file is missing");
        }
        let path = sandbox_settings
            .to_str()
            .context("the sandbox settings path is not UTF-8")?;
        // The `--` stops srt claiming claude's own flags.
        cmd.push_str(&format!(
            "npx --yes {SANDBOX_RUNTIME_PACKAGE} --settings {} -- ",
            launch_prompt::shell_quote(path)
        ));
    }
    // Tests point it at a fake (`WORKBENCH_CLAUDE_BIN`).
    match std::env::var("WORKBENCH_CLAUDE_BIN") {
        Ok(bin) if !bin.is_empty() => cmd.push_str(&launch_prompt::shell_quote(&bin)),
        _ => cmd.push_str("claude"),
    }
    if let Some(mode) = launch_mode(session.permission_mode.as_deref(), settings) {
        cmd.push_str(&format!(" --permission-mode {mode}"));
    }
    let flag = if session.resume {
        "--resume"
    } else {
        "--session-id"
    };
    cmd.push_str(&format!(" {flag} {}", session.id));
    // An entry id from the session file; only a UUID reaches the shell, unquoted
    // (cmd.exe keeps single quotes in the argument).
    if let Some(at) = session
        .resume_at
        .as_deref()
        .filter(|at| session.resume && is_uuid(at))
    {
        cmd.push_str(&format!(" --resume-session-at={at}"));
    }
    // `--` ends Claude's options, so a prompt like `--dangerously-skip-permissions`
    // is the prompt, never a flag (verified on Claude Code 2.1.292).
    if let Some(arg) = new_prompt(session).as_deref().and_then(launch_prompt::arg) {
        cmd.push_str(" -- ");
        cmd.push_str(&arg);
    }
    Ok(cmd)
}

/// Why a new session's prompt was left out of its command, to tell the person.
pub fn prompt_notice(session: &ClaudeSessionLaunch) -> Option<String> {
    let prompt = new_prompt(session)?;
    launch_prompt::arg(&prompt)
        .is_none()
        .then(|| launch_prompt::dropped_notice("Claude"))
}

/// The prompt a new session starts with; a resumed one has its conversation.
fn new_prompt(session: &ClaudeSessionLaunch) -> Option<String> {
    launch_prompt::normalize(session.prompt.as_deref().filter(|_| !session.resume))
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

    fn launch(resume: bool) -> ClaudeSessionLaunch {
        ClaudeSessionLaunch {
            id: SID.into(),
            resume,
            ..Default::default()
        }
    }

    fn command(session: &ClaudeSessionLaunch, mode: &str) -> Result<String> {
        terminal_command(session, &settings(mode, false), Path::new("/x"))
    }

    #[test]
    fn a_picked_mode_overrides_the_setting_even_when_default() {
        let cmd = |mode: Option<&str>| {
            let session = ClaudeSessionLaunch {
                permission_mode: mode.map(str::to_string),
                ..launch(true)
            };
            command(&session, "bypassPermissions").unwrap()
        };
        assert!(cmd(Some("default")).contains(" --permission-mode default "));
        assert!(cmd(Some("auto")).contains(" --permission-mode auto "));
        assert!(cmd(None).contains(" --permission-mode bypassPermissions "));
        assert!(!cmd(Some("yolo")).contains("--permission-mode"));
    }

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
    fn a_terminal_runs_its_command_or_claude_never_both() {
        let s = settings("default", false);
        let shell = Some("ls".to_string());
        assert_eq!(
            startup_command(shell.clone(), None, &s, &[]).unwrap(),
            shell
        );
        assert!(startup_command(shell, Some(&launch(true)), &s, &[]).is_err());
        let bad = ClaudeSessionLaunch {
            id: "not-a-uuid".into(),
            ..launch(true)
        };
        assert!(startup_command(None, Some(&bad), &s, &[]).is_err());
    }

    #[test]
    fn default_mode_adds_no_flag() {
        let cmd = command(&launch(true), "default");
        assert_eq!(cmd.unwrap(), format!("claude --resume {SID}"));
    }

    #[test]
    fn known_modes_are_passed_and_unknown_ones_dropped() {
        let cmd = command(&launch(false), "acceptEdits");
        assert_eq!(
            cmd.unwrap(),
            format!("claude --permission-mode acceptEdits --session-id {SID}")
        );
        let cmd = command(&launch(true), "rm -rf /");
        assert_eq!(cmd.unwrap(), format!("claude --resume {SID}"));
    }

    #[test]
    fn rejects_a_non_uuid_id() {
        let bad = ClaudeSessionLaunch {
            id: format!("{SID}; rm -rf ~"),
            ..launch(true)
        };
        assert!(command(&bad, "default").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_new_session_submits_its_prompt_as_one_quoted_argument() {
        let session = ClaudeSessionLaunch {
            prompt: Some("  it's $(broken)\r\nfix it  ".into()),
            ..launch(false)
        };
        assert_eq!(
            command(&session, "default").unwrap(),
            format!("claude --session-id {SID} -- 'it'\"'\"'s $(broken)\nfix it'")
        );
        assert_eq!(prompt_notice(&session), None);
    }

    #[test]
    fn a_prompt_that_looks_like_a_flag_stays_the_prompt() {
        for flag in [
            "--dangerously-skip-permissions",
            "--permission-mode=bypassPermissions",
        ] {
            let session = ClaudeSessionLaunch {
                prompt: Some(flag.into()),
                ..launch(false)
            };
            let cmd = command(&session, "default").unwrap();
            let (options, prompt) = cmd.split_once(" -- ").expect("`--` ends the options");
            assert!(!options.contains(flag), "{cmd}");
            assert!(prompt.contains(flag), "{cmd}");
        }
    }

    #[test]
    fn a_resume_or_a_blank_prompt_sends_none() {
        let resume = ClaudeSessionLaunch {
            prompt: Some("review".into()),
            ..launch(true)
        };
        assert_eq!(
            command(&resume, "default").unwrap(),
            format!("claude --resume {SID}")
        );
        let blank = ClaudeSessionLaunch {
            prompt: Some(" \n ".into()),
            ..launch(false)
        };
        assert_eq!(
            command(&blank, "default").unwrap(),
            format!("claude --session-id {SID}")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_starts_without_a_prompt_that_could_escape_its_quotes() {
        let ok = ClaudeSessionLaunch {
            prompt: Some("review this PR".into()),
            ..launch(false)
        };
        assert!(command(&ok, "default")
            .unwrap()
            .ends_with(" -- \"review this PR\""));
        for bad in ["a\" & calc", "%PATH%", "$(calc)", "a\nb"] {
            let session = ClaudeSessionLaunch {
                prompt: Some(bad.into()),
                ..launch(false)
            };
            assert_eq!(
                command(&session, "default").unwrap(),
                format!("claude --session-id {SID}"),
                "{bad}"
            );
            assert!(prompt_notice(&session).is_some(), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn sandbox_wraps_with_a_quoted_settings_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("it's.json");
        std::fs::write(&file, "{}").unwrap();
        let cmd = terminal_command(&launch(true), &settings("plan", true), &file).unwrap();
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
        let rewind = |at: &str| ClaudeSessionLaunch {
            resume_at: Some(at.into()),
            ..launch(true)
        };
        let cmd = command(&rewind(at), "default").unwrap();
        assert!(cmd.ends_with(&format!("--resume {SID} --resume-session-at={at}")));
        let cmd = command(&rewind("x; rm -rf /"), "default").unwrap();
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
            &launch(true),
            &settings("default", true),
            &dir.path().join("no.json"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("sandbox settings file is missing"));
    }

    /// No desktop needed: a launch writes the file from the host's settings and
    /// projects (a standalone server's case).
    #[cfg(unix)]
    #[test]
    fn a_sandboxed_launch_writes_the_settings_file_itself() {
        let config = crate::paths::test_config_dir();
        let project = tempfile::tempdir().unwrap();
        let project_path = project.path().to_string_lossy().to_string();
        let projects = [ProjectConfig {
            name: "p".into(),
            path: project_path.clone(),
            group: None,
            shell: None,
            startup_command: None,
            tasks: Vec::new(),
            claude_account_id: None,
        }];
        let file = config.join("sandbox-runtime.json");
        let _ = std::fs::remove_file(&file);

        let cmd = startup_command(
            None,
            Some(&launch(true)),
            &settings("default", true),
            &projects,
        )
        .unwrap()
        .unwrap();

        assert!(cmd.contains(&launch_prompt::shell_quote(file.to_str().unwrap())), "{cmd}");
        let written = std::fs::read_to_string(&file).unwrap();
        let canonical = std::fs::canonicalize(&project_path).unwrap();
        assert!(written.contains(canonical.to_str().unwrap()), "{written}");
    }
}
