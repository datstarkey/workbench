//! Spawning child processes and shells, with the per-platform differences in
//! one place. Used by the desktop `PtyManager`, the server's `TerminalManager`,
//! and every `git`/`gh` invocation.

use std::ffi::OsStr;
use std::io::Write;
use std::process::Command;

/// A `std::process::Command` that never flashes a console window.
///
/// Release builds link with `windows_subsystem = "windows"`, so the app owns no
/// console — Windows therefore allocates and *shows* a fresh console window for
/// every console child it spawns. Since `git`/`gh` run on a poll loop, that's a
/// blank window popping up every few seconds. `CREATE_NO_WINDOW` suppresses it.
///
/// Use this instead of `Command::new` for every child process.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// The shell to spawn for a terminal when the project configures none.
///
/// The fallback only fires when `$SHELL`/`%COMSPEC%` is unset — routine for the
/// headless server under systemd or in a container, where it must name a shell
/// that is actually present. Hence `/bin/sh` rather than zsh/bash on non-macOS
/// Unix: it is the one binary POSIX guarantees, and slim images often ship
/// neither of the others.
pub fn default_shell() -> String {
    #[cfg(target_os = "macos")]
    const UNIX_FALLBACK: &str = "/bin/zsh";
    #[cfg(all(unix, not(target_os = "macos")))]
    const UNIX_FALLBACK: &str = "/bin/sh";

    #[cfg(unix)]
    {
        std::env::var("SHELL").unwrap_or_else(|_| UNIX_FALLBACK.to_string())
    }
    #[cfg(windows)]
    {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".to_string())
    }
}

/// Arguments that make the spawned shell a login shell. Empty on Windows —
/// `cmd.exe` and `powershell.exe` have no equivalent, and passing `-l` makes the
/// spawn fail outright.
pub fn login_args() -> &'static [&'static str] {
    #[cfg(unix)]
    {
        &["-l"]
    }
    #[cfg(windows)]
    {
        &[]
    }
}

#[cfg(unix)]
const INHERITED_KEYS: &[&str] = &["PATH", "HOME", "USER", "LANG", "SHELL", "LOGNAME"];

#[cfg(windows)]
const INHERITED_KEYS: &[&str] = &[
    "PATH",
    "PATHEXT",
    "USERPROFILE",
    "USERNAME",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "SystemRoot",
    "SystemDrive",
    "COMSPEC",
    "TEMP",
    "TMP",
];

/// Environment a spawned shell needs inherited from the host process, plus the
/// terminal-capability vars xterm.js renders against.
pub fn inherited_env() -> Vec<(&'static str, String)> {
    let mut env: Vec<(&'static str, String)> = INHERITED_KEYS
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|val| (*key, val)))
        .collect();

    #[cfg(unix)]
    if !env.iter().any(|(key, _)| *key == "LANG") {
        env.push(("LANG", "en_US.UTF-8".to_string()));
    }

    env.push(("TERM", "xterm-256color".to_string()));
    env.push(("COLORTERM", "truecolor".to_string()));
    env
}

/// Submit a command line to a shell running in a PTY.
///
/// Terminated with CR, not LF: CR is what pressing Enter actually sends. Unix
/// ttys map it to NL on input, while a Windows console only submits on CR — a
/// bare LF leaves the command sitting at the prompt, unexecuted. Every startup
/// command goes through here so the two can't drift apart again.
pub fn submit_line(writer: &mut dyn Write, line: &str) -> std::io::Result<()> {
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\r")?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_is_nonempty() {
        assert!(!default_shell().is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn unix_shell_is_a_path_and_logs_in() {
        assert!(default_shell().starts_with('/'));
        assert_eq!(login_args(), &["-l"]);
    }

    #[test]
    #[cfg(windows)]
    fn windows_shell_is_a_known_shell_with_no_login_args() {
        let shell = default_shell().to_lowercase();
        assert!(shell.contains("cmd") || shell.contains("powershell") || shell.contains("pwsh"));
        assert!(login_args().is_empty());
    }

    #[test]
    fn submit_line_terminates_with_cr() {
        let mut out: Vec<u8> = Vec::new();
        submit_line(&mut out, "claude").unwrap();
        assert_eq!(out, b"claude\r");
    }

    #[test]
    fn inherited_env_carries_path_and_term() {
        let env = inherited_env();
        assert!(env.iter().any(|(k, v)| *k == "PATH" && !v.is_empty()));
        assert!(env.iter().any(|(k, v)| *k == "TERM" && v == "xterm-256color"));
    }
}
