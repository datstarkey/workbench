//! Spawning child processes and shells, with the per-platform differences in
//! one place. Used by the desktop native terminals, the server's `TerminalManager`,
//! and every `git`/`gh` invocation.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
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

/// A CLI tool (`git`, `gh`, `claude`, ...) by name or path, found on the
/// enriched search path (GUI apps don't inherit the shell's PATH), which its
/// children get too.
///
/// Never a bare name with PATH changed: std then forks to search PATH itself
/// instead of using `posix_spawn`, and a fork in this large, many-threaded
/// process holds the allocator's locks long enough to stall every thread
/// (the git/gh polls froze the whole app, servers included, for minutes). A
/// tool that isn't on the enriched path runs by name on the app's own PATH.
pub fn tool(program: impl AsRef<OsStr>) -> Command {
    let program = program.as_ref();
    let path = std::path::Path::new(program);
    let found = if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        program.to_str().and_then(|name| {
            let exe = if cfg!(windows) && path.extension().is_none() {
                format!("{name}.exe")
            } else {
                name.to_string()
            };
            crate::paths::find_on_path(&[&exe])
        })
    };
    match found {
        Some(full) => {
            let mut cmd = command(full);
            cmd.env("PATH", crate::paths::enriched_path());
            cmd
        }
        None => command(program),
    }
}

/// How long to keep reading a child's pipes after it exits, for a process it started
/// that still holds them (an ssh ControlPersist master, an MCP server).
const PIPE_GRACE: Duration = Duration::from_millis(250);

/// Run `cmd` to completion, killing it at `timeout` (an `ErrorKind::TimedOut` error).
/// Stdout and stderr are drained on threads while it runs, so a chatty child can't
/// stall on a full pipe; stdin is null, so it can't wait on input either. On Unix the
/// child gets its own process group and the whole group is killed at the deadline, so
/// hooks, ssh and helpers it started don't outlive it.
pub fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // std keeps using posix_spawn with a process group (POSIX_SPAWN_SETPGROUP).
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(cmd, 0);
    let mut child = cmd.spawn()?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let deadline = Instant::now() + timeout;
    let mut pause = Duration::from_millis(1);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            #[cfg(unix)]
            if let Ok(pgid) = libc::pid_t::try_from(child.id()) {
                // SAFETY: plain syscall on the group this child leads.
                unsafe { libc::kill(-pgid, libc::SIGKILL) };
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("timed out after {}s", timeout.as_secs_f32()),
            ));
        }
        std::thread::sleep(pause.min(deadline.saturating_duration_since(Instant::now())));
        pause = (pause * 2).min(Duration::from_millis(20));
    };
    let until = (Instant::now() + PIPE_GRACE).min(deadline.max(Instant::now()));
    Ok(Output {
        status,
        stdout: collect(&stdout, until),
        stderr: collect(&stderr, until),
    })
}

/// Read `pipe` on a thread, sending each chunk as it arrives; the channel closes at EOF.
fn drain(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    if let Some(mut pipe) = pipe {
        std::thread::spawn(move || {
            let mut buf = vec![0; 64 * 1024];
            while let Ok(n @ 1..) = pipe.read(&mut buf) {
                if tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
    }
    rx
}

/// Everything drained so far, waiting for EOF no later than `until`.
fn collect(rx: &mpsc::Receiver<Vec<u8>>, until: Instant) -> Vec<u8> {
    let mut out = Vec::new();
    while let Ok(chunk) = rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
        out.extend_from_slice(&chunk);
    }
    out
}

/// Run `cmd` and return its stdout if it exits successfully within `timeout`;
/// `None` if it can't start, fails, or is killed at the deadline.
pub fn output_with_timeout(cmd: &mut Command, timeout: Duration) -> Option<String> {
    let out = run_with_timeout(cmd, timeout).ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
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

/// Only plain http(s) and mailto URLs reach the OS opener: it would just as
/// happily run a local file path, and a raw space, quote or control char never
/// appears in a valid URL.
fn validate_open_url(url: &str) -> anyhow::Result<()> {
    let lower = url.to_ascii_lowercase();
    if !["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
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

/// Open an http(s) or mailto URL in its default handler.
///
/// Windows uses the `open` crate's `ShellExecuteExW` (feature
/// `shellexecute-on-windows`): Unicode-safe, no `cmd` parsing (`cmd /c start ""
/// <url>` opened `\\` because Rust escapes the `""` title as `"\"\""`, and cmd
/// splits URLs on `&`), and failures come back as errors. Elsewhere it runs
/// `open`/`xdg-open` itself: the crate's `that_detached` setsids in a
/// `pre_exec`, so it forks (see [`tool`]). The URL starts with its scheme, so
/// it can't pose as an option.
pub fn open_url(url: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    validate_open_url(url)?;
    #[cfg(windows)]
    {
        open::that_detached(url).context("Failed to open URL")
    }
    #[cfg(unix)]
    {
        #[cfg(target_os = "macos")]
        let mut cmd = command("/usr/bin/open");
        #[cfg(not(target_os = "macos"))]
        let mut cmd = {
            use std::os::unix::process::CommandExt;
            let mut cmd = tool("xdg-open");
            // Its own group (posix_spawn does that too): a browser it starts isn't ours.
            cmd.process_group(0);
            cmd
        };
        cmd.arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        spawn_detached(&mut cmd).context("Failed to open URL")
    }
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
    fn run_with_timeout_kills_a_hung_child() {
        let started = Instant::now();
        let err =
            run_with_timeout(command("sleep").arg("10"), Duration::from_millis(200)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn run_with_timeout_drains_output_bigger_than_a_pipe() {
        let out = run_with_timeout(
            command("sh").args(["-c", "head -c 300000 /dev/zero; echo err >&2"]),
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout.len(), 300_000);
        assert_eq!(out.stderr, b"err\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_tool_never_has_a_bare_name_with_path_set() {
        // std forks (no posix_spawn) for exactly that pair.
        let sets_path = |cmd: &Command| cmd.get_envs().any(|(k, _)| k == "PATH");
        let found = tool("sh");
        assert!(std::path::Path::new(found.get_program()).is_absolute());
        assert!(
            sets_path(&found),
            "a found tool's children get the enriched PATH"
        );

        let missing = tool("workbench-no-such-tool");
        assert_eq!(missing.get_program(), "workbench-no-such-tool");
        assert!(!sets_path(&missing));

        let out = tool("sh").args(["-c", "echo ok"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    #[cfg(unix)]
    #[test]
    fn run_with_timeout_drains_large_output_and_kills_at_the_deadline() {
        // Well past a pipe buffer on both streams: a child that isn't drained blocks.
        let out = run_with_timeout(
            tool("sh").args([
                "-c",
                "head -c 1000000 /dev/zero; head -c 300000 /dev/zero >&2",
            ]),
            Duration::from_secs(20),
        )
        .unwrap();
        assert!(out.status.success());
        assert_eq!((out.stdout.len(), out.stderr.len()), (1_000_000, 300_000));

        let started = Instant::now();
        let err =
            run_with_timeout(tool("sleep").arg("30"), Duration::from_millis(200)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));

        assert_eq!(
            output_with_timeout(tool("sh").args(["-c", "echo hi"]), Duration::from_secs(20))
                .as_deref(),
            Some("hi\n")
        );
        assert!(output_with_timeout(&mut tool("false"), Duration::from_secs(20)).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn run_with_timeout_returns_when_a_background_process_holds_the_pipes() {
        let started = Instant::now();
        let out = run_with_timeout(
            tool("sh").args(["-c", "echo done; sleep 30 &"]),
            Duration::from_secs(20),
        )
        .unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout), "done\n");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn run_with_timeout_kills_the_whole_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("survived");
        let script = format!("(sleep 1; touch '{}') & sleep 30", marker.display());
        let err = run_with_timeout(tool("sh").args(["-c", &script]), Duration::from_millis(200))
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        std::thread::sleep(Duration::from_millis(1500));
        assert!(!marker.exists(), "a grandchild outlived the timeout");
    }

    #[test]
    fn open_url_accepts_only_http_urls() {
        assert!(validate_open_url("https://github.com/o/r/compare/main...x?expand=1&a=b").is_ok());
        assert!(validate_open_url("HTTP://example.com").is_ok());
        assert!(validate_open_url("https://trello.com/c/x/1-café").is_ok());
        assert!(validate_open_url("\\\\").is_err());
        assert!(validate_open_url("C:\\Windows\\System32\\calc.exe").is_err());
        assert!(validate_open_url("file:///etc/passwd").is_err());
        assert!(validate_open_url("mailto:someone@example.com").is_ok());
        assert!(validate_open_url("-a Calculator").is_err());
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
