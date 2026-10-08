use super::*;
use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

fn pair() -> PtyPair {
    native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap()
}

fn sh(script: &str) -> CommandBuilder {
    let mut cmd = CommandBuilder::new("sh");
    cmd.args(["-c", script]);
    cmd
}

/// Everything the shell prints until it exits (fails the test after 10s).
fn output(pair: PtyPair, child: &mut Box<dyn Child + Send + Sync>) -> String {
    let mut reader = pair.master.try_clone_reader().unwrap();
    drop(pair.slave);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        let _ = tx.send(String::from_utf8_lossy(&out).into_owned());
    });
    let out = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("shell never exited");
    child.wait().unwrap();
    drop(pair.master);
    out
}

fn field(out: &str, key: &str) -> String {
    out.lines()
        .find_map(|l| l.split_once(key).map(|(_, value)| value))
        .unwrap_or_else(|| panic!("no {key} in {out:?}"))
        .trim()
        .to_string()
}

#[test]
fn shell_leads_a_session_on_the_pty_with_its_env_and_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let p = pair();
    let mut cmd = sh(
        "echo tty=$(tty); echo ids=$$ $(ps -o pgid= -o tpgid= -p $$); \
         echo size=$(stty size); echo cwd=$(pwd -P); echo foo=$FOO; exit 7",
    );
    cmd.cwd(dir.path());
    cmd.env("FOO", "bar baz");
    let mut child = spawn(&p, cmd).unwrap();
    let out = output(p, &mut child);

    assert!(field(&out, "tty=").starts_with("/dev/"), "{out}");
    let ids = field(&out, "ids=");
    let ids: Vec<&str> = ids.split_whitespace().collect();
    let pid = child.process_id().unwrap().to_string();
    // Group leader (setsid) and the PTY's foreground group: its ctty.
    assert_eq!(ids, [pid.as_str(); 3], "{out}");
    assert_eq!(field(&out, "size="), "24 80");
    assert_eq!(
        field(&out, "cwd="),
        dir.path().canonicalize().unwrap().to_string_lossy()
    );
    assert_eq!(field(&out, "foo="), "bar baz");
    assert_eq!(child.try_wait().unwrap().unwrap().exit_code(), 7);
}

/// Without a shell in the way: bash reopens its tty at startup, which
/// claims a controlling terminal by itself and would hide a missing one
/// (zsh doesn't, and then has no job control).
#[test]
fn the_pty_is_the_controlling_terminal_before_any_shell_runs() {
    let p = pair();
    let mut cmd = CommandBuilder::new("perl");
    cmd.args(["-e", "exec qw(ps -o pid= -o pgid= -o tpgid= -p), $$"]);
    let mut child = spawn(&p, cmd).unwrap();
    let out = output(p, &mut child);
    let pid = child.process_id().unwrap().to_string();
    let ids: Vec<&str> = out.split_whitespace().collect();
    assert_eq!(ids, [pid.as_str(); 3], "{out}");
}

#[test]
fn a_missing_cwd_falls_back_to_home() {
    let home = tempfile::tempdir().unwrap();
    let p = pair();
    let mut cmd = sh("echo cwd=$(pwd -P)");
    cmd.cwd("/definitely/not/here");
    cmd.env("HOME", home.path());
    let mut child = spawn(&p, cmd).unwrap();
    let out = output(p, &mut child);
    assert_eq!(
        field(&out, "cwd="),
        home.path().canonicalize().unwrap().to_string_lossy()
    );
}

#[test]
fn resize_reaches_the_shell() {
    let p = pair();
    let mut child = spawn(&p, sh("read _; echo size=$(stty size)")).unwrap();
    p.master
        .resize(PtySize {
            rows: 50,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    p.master.take_writer().unwrap().write_all(b"\n").unwrap();
    let out = output(p, &mut child);
    assert_eq!(field(&out, "size="), "50 120");
}

#[test]
fn job_control_works() {
    let p = pair();
    // `set -m` needs a controlling terminal: each job gets its own group.
    let script = "set -m; sleep 30 & job=$!; echo job=$job $(ps -o pgid= -p $job); \
                  kill -TERM -$job; wait $job; echo status=$?";
    let mut child = spawn(&p, sh(script)).unwrap();
    let out = output(p, &mut child);
    let job = field(&out, "job=");
    let job: Vec<&str> = job.split_whitespace().collect();
    assert_eq!(job[0], job[1], "{out}");
    assert_eq!(field(&out, "status="), "143");
}

#[test]
fn kill_ends_the_shell_and_a_signal_is_not_success() {
    let p = pair();
    let mut child = spawn(&p, sh("trap '' HUP; read _")).unwrap();
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
    assert!(status.to_string().starts_with("Terminated by"), "{status}");
}

#[test]
fn a_missing_program_is_an_error() {
    let p = pair();
    assert!(spawn(&p, CommandBuilder::new("/no/such/shell")).is_err());
    assert!(spawn(&p, CommandBuilder::new("no-such-shell-anywhere")).is_err());
}

static FORKS: AtomicUsize = AtomicUsize::new(0);
extern "C" fn count_fork() {
    FORKS.fetch_add(1, Ordering::SeqCst);
}

/// The point of this module: no fork (atfork handlers run on every fork,
/// never on posix_spawn). portable-pty's own spawn is the control.
#[test]
fn spawning_never_forks() {
    unsafe { libc::pthread_atfork(Some(count_fork), None, None) };
    let p = pair();
    let before = FORKS.load(Ordering::SeqCst);
    let mut child = spawn(&p, sh("exit 0")).unwrap();
    assert_eq!(FORKS.load(Ordering::SeqCst), before, "spawn forked");
    output(p, &mut child);

    let p = pair();
    let mut control = p.slave.spawn_command(sh("exit 0")).unwrap();
    assert!(
        FORKS.load(Ordering::SeqCst) > before,
        "the control didn't fork"
    );
    output(p, &mut control);
}

#[test]
fn a_bare_program_never_runs_from_the_cwd() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let planted = dir.path().join("sh");
    std::fs::write(&planted, "#!/bin/sh\necho planted\n").unwrap();
    std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o755)).unwrap();
    let p = pair();
    let mut cmd = sh("echo real");
    cmd.cwd(dir.path());
    let mut child = spawn(&p, cmd).unwrap();
    let out = output(p, &mut child);
    assert!(out.contains("real") && !out.contains("planted"), "{out}");
}
