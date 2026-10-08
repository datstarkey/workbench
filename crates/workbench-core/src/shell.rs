//! Spawning child processes and shells, with the per-platform differences in
//! one place. Used by the desktop native terminals, the server's `TerminalManager`,
//! and every `git`/`gh` invocation.

use std::ffi::OsStr;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

/// A CLI tool (`git`, `gh`, ...) found on the enriched search path (GUI apps
/// don't inherit the shell's PATH), which its children get too.
///
/// It is spawned by its full path: a bare name with PATH changed makes std
/// fork and search PATH itself instead of using `posix_spawn`, and a fork in
/// this large, many-threaded process holds the allocator's locks long enough
/// to stall every thread. Under the git/gh polls that froze the whole app,
/// servers included, for minutes.
pub fn tool(program: &str) -> Command {
    let mut cmd = command(resolve(program));
    cmd.env("PATH", crate::paths::enriched_path());
    cmd
}

/// `program`'s full path on the enriched search path, looked up once; the bare
/// name if it isn't there (not cached, so a later install is found).
fn resolve(program: &str) -> std::path::PathBuf {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    static FOUND: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    let found = FOUND.get_or_init(Default::default);
    if let Some(path) = found.lock().unwrap_or_else(|e| e.into_inner()).get(program) {
        return path.clone();
    }
    let names = if cfg!(windows) {
        vec![format!("{program}.exe"), program.to_string()]
    } else {
        vec![program.to_string()]
    };
    let path = std::env::split_paths(&crate::paths::enriched_path())
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|p| p.is_file());
    match path {
        Some(path) => {
            found
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(program.to_string(), path.clone());
            path
        }
        None => PathBuf::from(program),
    }
}

/// Run `cmd` and return its stdout if it exits successfully within `timeout`;
/// `None` if it can't start, fails, or is killed at the deadline. Stdout is read
/// after exit, so only for commands with small output.
pub fn output_with_timeout(cmd: &mut Command, timeout: Duration) -> Option<String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(child.stdout.as_mut()?, &mut out).ok()?;
    Some(out)
}

/// Spawn a fire-and-forget child (`open`, `xdg-open`, …) and reap it on a
/// background thread. Dropping a `Child` never waits on it, so without this each
/// launch leaves a zombie in the process table for the lifetime of the app.
pub fn spawn_detached(cmd: &mut Command) -> std::io::Result<()> {
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Only plain http(s) URLs reach the OS opener: it would just as happily run a
/// local file path, and a raw space, quote or control char never appears in a
/// valid URL.
fn validate_open_url(url: &str) -> anyhow::Result<()> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        anyhow::bail!("Refusing to open non-http(s) URL: {url:?}");
    }
    if url
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '"')
    {
        anyhow::bail!("Refusing to open URL with whitespace, quote or control characters: {url:?}");
    }
    Ok(())
}

/// Open an http(s) URL in the default browser.
///
/// Uses the `open` crate rather than spawning an opener ourselves. On Windows
/// that is `ShellExecuteExW` (feature `shellexecute-on-windows`): Unicode-safe,
/// no `cmd` parsing (`cmd /c start "" <url>` opened `\\` because Rust escapes
/// the `""` title as `"\"\""`, and cmd splits URLs on `&`), and failures come
/// back as errors instead of vanishing in a detached child.
pub fn open_url(url: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    validate_open_url(url)?;
    open::that_detached(url).context("Failed to open URL")
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

    #[cfg(unix)]
    #[test]
    fn a_tool_is_spawned_by_its_full_path() {
        // A full path is what lets std use posix_spawn rather than fork.
        assert!(resolve("sh").is_absolute());
        assert_eq!(resolve("sh"), resolve("sh"), "looked up once");
        assert_eq!(
            resolve("workbench-no-such-tool"),
            std::path::PathBuf::from("workbench-no-such-tool")
        );
        let out = tool("sh").args(["-c", "echo ok"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    #[test]
    fn open_url_accepts_only_http_urls() {
        assert!(validate_open_url("https://github.com/o/r/compare/main...x?expand=1&a=b").is_ok());
        assert!(validate_open_url("HTTP://example.com").is_ok());
        assert!(validate_open_url("https://trello.com/c/x/1-café").is_ok());
        assert!(validate_open_url("\\\\").is_err());
        assert!(validate_open_url("C:\\Windows\\System32\\calc.exe").is_err());
        assert!(validate_open_url("file:///etc/passwd").is_err());
        assert!(validate_open_url("https://github.com/a b").is_err());
        assert!(validate_open_url("https://github.com/a\"b").is_err());
        assert!(validate_open_url("https://github.com/\u{1b}[31m").is_err());
    }

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
        assert!(env
            .iter()
            .any(|(k, v)| *k == "TERM" && v == "xterm-256color"));
    }
}
