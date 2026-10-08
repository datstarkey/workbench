//! Starting a terminal's shell on a PTY without forking this process.
//!
//! portable-pty's `spawn_command` gives std a `pre_exec` closure (setsid,
//! TIOCSCTTY, closing stray fds), and any `pre_exec` makes std `fork` instead of
//! `posix_spawn`. A fork in this large, many-threaded process holds the
//! allocator's locks long enough to stall every thread (see [`crate::shell::tool`]),
//! and restoring a workspace opens many panes at once. So on Unix the child is
//! started with `posix_spawn` directly: `POSIX_SPAWN_SETSID` makes it a session
//! (and process group) leader, and the slave is opened by path in the file
//! actions without `O_NOCTTY`. On Linux that open, after glibc's setsid, makes
//! the slave the controlling terminal, as portable-pty's TIOCSCTTY did. XNU runs
//! file actions in the parent's context, so on macOS the child is a tiny shim
//! (`pty_exec.c`, built by `build.rs`, embedded and written to the user cache
//! dir) that calls TIOCSCTTY and execs the shell. Windows (ConPTY) never forks
//! and keeps portable-pty's spawn.

use anyhow::Result;
use portable_pty::{Child, CommandBuilder, PtyPair};

/// Start `cmd` with the pair's slave as its stdio and controlling terminal.
/// Same contract as `pair.slave.spawn_command(cmd)`: argv[0] kept, the program
/// found on the command's own PATH, its cwd (home when it isn't a directory),
/// exactly its environment plus `SHELL` when unset, and a new session.
pub fn spawn(pair: &PtyPair, cmd: CommandBuilder) -> Result<Box<dyn Child + Send + Sync>> {
    #[cfg(unix)]
    {
        if let Some(child) = unix::spawn(pair, &cmd)? {
            return Ok(Box::new(child));
        }
        log::warn!("posix_spawn lacks setsid/chdir support here; spawning the PTY child with fork");
    }
    pair.slave.spawn_command(cmd)
}

#[cfg(unix)]
mod unix {
    use std::ffi::{CStr, CString, OsStr};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::{io, mem, ptr};

    use anyhow::{anyhow, bail, Context, Result};
    use libc::{c_char, c_int, c_short, pid_t, posix_spawn_file_actions_t, posix_spawnattr_t};
    use portable_pty::{Child, ChildKiller, CommandBuilder, ExitStatus, PtyPair};

    // spawn.h values; the libc crate has no Apple `POSIX_SPAWN_SETSID`.
    #[cfg(target_os = "macos")]
    const POSIX_SPAWN_SETSID: c_short = 0x0400;
    #[cfg(not(target_os = "macos"))]
    const POSIX_SPAWN_SETSID: c_short = libc::POSIX_SPAWN_SETSID as c_short;

    /// Reset in the child, as portable-pty's `pre_exec` did (plus SIGPIPE, which
    /// std resets for every child because Rust ignores it).
    const DEFAULT_SIGNALS: [c_int; 7] = [
        libc::SIGCHLD,
        libc::SIGHUP,
        libc::SIGINT,
        libc::SIGQUIT,
        libc::SIGTERM,
        libc::SIGALRM,
        libc::SIGPIPE,
    ];

    type AddChdir = unsafe extern "C" fn(*mut posix_spawn_file_actions_t, *const c_char) -> c_int;
    #[cfg(not(target_os = "macos"))]
    type AddClosefrom = unsafe extern "C" fn(*mut posix_spawn_file_actions_t, c_int) -> c_int;

    /// Looked up at run time, as std does: glibc gained `addchdir_np` in 2.29
    /// and `addclosefrom_np` in 2.34, and linking them would stop the
    /// standalone server starting on older distributions.
    fn symbol(name: &CStr) -> Option<*mut libc::c_void> {
        let sym = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
        (!sym.is_null()).then_some(sym)
    }

    struct FileActions(posix_spawn_file_actions_t);
    impl Drop for FileActions {
        fn drop(&mut self) {
            unsafe { libc::posix_spawn_file_actions_destroy(&mut self.0) };
        }
    }

    struct Attr(posix_spawnattr_t);
    impl Drop for Attr {
        fn drop(&mut self) {
            unsafe { libc::posix_spawnattr_destroy(&mut self.0) };
        }
    }

    fn check(code: c_int, what: &str) -> Result<()> {
        if code == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(code)).context(what.to_string())
        }
    }

    fn cstring(bytes: &[u8]) -> Result<CString> {
        CString::new(bytes).map_err(|_| anyhow!("NUL byte in {:?}", String::from_utf8_lossy(bytes)))
    }

    /// The shim's bytes (see `build.rs`).
    #[cfg(target_os = "macos")]
    const EXEC_SHIM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/workbench-pty-exec"));

    /// The shim on disk, (re)written when missing or changed. Named by its
    /// content, so app versions running side by side never swap it under each
    /// other.
    #[cfg(target_os = "macos")]
    fn exec_shim() -> Result<PathBuf> {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        let path = PATH.get_or_init(|| {
            use std::hash::{Hash, Hasher};
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            EXEC_SHIM.hash(&mut hash);
            dirs::cache_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("workbench")
                .join(format!("workbench-pty-exec-{:016x}", hash.finish()))
        });
        let ready = fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
            && fs::read(path).is_ok_and(|bytes| bytes == EXEC_SHIM);
        if !ready {
            let dir = path.parent().context("shim path has no parent")?;
            fs::create_dir_all(dir)?;
            let tmp = dir.join(format!(
                ".pty-exec-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            fs::write(&tmp, EXEC_SHIM)?;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
            fs::rename(&tmp, path).context("Failed to install the terminal exec shim")?;
        }
        Ok(path.clone())
    }

    /// The slave's device path, for the child to open (on Linux only an open,
    /// not an inherited fd, makes it the controlling terminal).
    fn slave_path(master: c_int) -> Result<CString> {
        let mut buf = [0 as c_char; 128];
        #[cfg(target_os = "macos")]
        let ok = unsafe { libc::ioctl(master, libc::TIOCPTYGNAME as _, buf.as_mut_ptr()) } == 0;
        #[cfg(not(target_os = "macos"))]
        let ok = unsafe { libc::ptsname_r(master, buf.as_mut_ptr(), buf.len()) } == 0;
        if !ok {
            return Err(io::Error::last_os_error()).context("Failed to name the PTY slave");
        }
        Ok(unsafe { CStr::from_ptr(buf.as_ptr()) }.to_owned())
    }

    fn executable(path: &Path) -> bool {
        CString::new(path.as_os_str().as_bytes())
            .is_ok_and(|p| unsafe { libc::access(p.as_ptr(), libc::X_OK) } == 0)
    }

    /// portable-pty's lookup: a relative program is tried in the cwd, then on
    /// the command's own PATH; spawning by absolute path keeps libc from
    /// searching the parent's.
    fn resolve(cmd: &CommandBuilder, program: &OsStr, cwd: &Path) -> Result<PathBuf> {
        let path = Path::new(program);
        if path.is_absolute() {
            if executable(path) {
                return Ok(path.to_path_buf());
            }
            bail!(
                "Unable to spawn {} because it doesn't exist or is not executable",
                path.display()
            );
        }
        let in_cwd = cwd.join(path);
        if in_cwd.exists() {
            return Ok(in_cwd);
        }
        cmd.get_env("PATH")
            .into_iter()
            .flat_map(std::env::split_paths)
            .map(|dir| dir.join(path))
            .find(|candidate| executable(candidate))
            .ok_or_else(|| {
                anyhow!(
                    "Unable to spawn {} because it was not found in PATH",
                    path.display()
                )
            })
    }

    /// `Ok(None)` when this libc can't do it without a fork (old glibc).
    pub(super) fn spawn(pair: &PtyPair, cmd: &CommandBuilder) -> Result<Option<PtyChild>> {
        let Some(add_chdir) = symbol(c"posix_spawn_file_actions_addchdir_np") else {
            return Ok(None);
        };
        let add_chdir: AddChdir = unsafe { mem::transmute(add_chdir) };

        let master = pair.master.as_raw_fd().context("PTY master has no fd")?;
        let tty = slave_path(master)?;

        let argv = cmd.get_argv();
        let program = argv.first().context("No program to spawn")?;
        let home = cmd
            .get_env("HOME")
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        let cwd = cmd
            .get_cwd()
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir())
            .unwrap_or(home);
        let exe = cstring(resolve(cmd, program, &cwd)?.as_os_str().as_bytes())?;
        let cwd = cstring(cwd.as_os_str().as_bytes())?;

        let args = argv
            .iter()
            .map(|a| cstring(a.as_bytes()))
            .collect::<Result<Vec<_>>>()?;
        #[cfg(target_os = "macos")]
        let (exe, args) = {
            let shim = cstring(exec_shim()?.as_os_str().as_bytes())?;
            let args = [shim.clone(), exe]
                .into_iter()
                .chain(args)
                .collect::<Vec<_>>();
            (shim, args)
        };
        let mut env = cmd
            .iter_full_env_as_str()
            .map(|(k, v)| cstring(format!("{k}={v}").as_bytes()))
            .collect::<Result<Vec<_>>>()?;
        if cmd.get_env("SHELL").is_none() {
            env.push(cstring(format!("SHELL={}", cmd.get_shell()).as_bytes())?);
        }
        let mut argv_ptrs: Vec<*mut c_char> = args.iter().map(|a| a.as_ptr() as *mut _).collect();
        argv_ptrs.push(ptr::null_mut());
        let mut env_ptrs: Vec<*mut c_char> = env.iter().map(|e| e.as_ptr() as *mut _).collect();
        env_ptrs.push(ptr::null_mut());

        unsafe {
            let mut actions = FileActions(mem::zeroed());
            check(
                libc::posix_spawn_file_actions_init(&mut actions.0),
                "file actions",
            )?;
            let fa = &mut actions.0 as *mut _;
            check(add_chdir(fa, cwd.as_ptr()), "chdir action")?;
            check(
                libc::posix_spawn_file_actions_addopen(fa, 0, tty.as_ptr(), libc::O_RDWR, 0),
                "open slave action",
            )?;
            check(
                libc::posix_spawn_file_actions_adddup2(fa, 0, 1),
                "dup2 action",
            )?;
            check(
                libc::posix_spawn_file_actions_adddup2(fa, 0, 2),
                "dup2 action",
            )?;
            // Close every other fd, as portable-pty's close_random_fds did (Apple:
            // POSIX_SPAWN_CLOEXEC_DEFAULT below). Everything is opened CLOEXEC
            // anyway, so a glibc without it loses nothing that matters.
            #[cfg(not(target_os = "macos"))]
            if let Some(closefrom) = symbol(c"posix_spawn_file_actions_addclosefrom_np") {
                let closefrom: AddClosefrom = mem::transmute(closefrom);
                check(closefrom(fa, 3), "closefrom action")?;
            }

            let mut attr = Attr(mem::zeroed());
            check(libc::posix_spawnattr_init(&mut attr.0), "spawn attributes")?;
            let mut flags = POSIX_SPAWN_SETSID
                | libc::POSIX_SPAWN_SETSIGDEF as c_short
                | libc::POSIX_SPAWN_SETSIGMASK as c_short;
            #[cfg(target_os = "macos")]
            {
                flags |= libc::POSIX_SPAWN_CLOEXEC_DEFAULT as c_short;
            }
            if libc::posix_spawnattr_setflags(&mut attr.0, flags) == libc::EINVAL {
                // glibc before 2.26 has no POSIX_SPAWN_SETSID.
                return Ok(None);
            }
            let mut set: libc::sigset_t = mem::zeroed();
            libc::sigemptyset(&mut set);
            check(
                libc::posix_spawnattr_setsigmask(&mut attr.0, &set),
                "signal mask",
            )?;
            for sig in DEFAULT_SIGNALS {
                libc::sigaddset(&mut set, sig);
            }
            check(
                libc::posix_spawnattr_setsigdefault(&mut attr.0, &set),
                "signal defaults",
            )?;

            let mut pid: pid_t = 0;
            check(
                libc::posix_spawn(
                    &mut pid,
                    exe.as_ptr(),
                    fa,
                    &attr.0,
                    argv_ptrs.as_ptr(),
                    env_ptrs.as_ptr(),
                ),
                "Failed to spawn shell",
            )?;
            Ok(Some(PtyChild { pid, status: None }))
        }
    }

    /// The shell, as portable-pty's `Child` for a `std::process::Child`:
    /// `kill` is SIGHUP, a short grace, then SIGKILL; the status is kept once
    /// reaped so a later wait never touches a recycled pid.
    #[derive(Debug)]
    pub(super) struct PtyChild {
        pid: pid_t,
        status: Option<ExitStatus>,
    }

    impl PtyChild {
        fn reap(&mut self, flags: c_int) -> io::Result<Option<ExitStatus>> {
            if let Some(status) = &self.status {
                return Ok(Some(status.clone()));
            }
            let mut raw = 0;
            loop {
                match unsafe { libc::waitpid(self.pid, &mut raw, flags) } {
                    0 => return Ok(None),
                    -1 => {
                        let err = io::Error::last_os_error();
                        if err.kind() != io::ErrorKind::Interrupted {
                            return Err(err);
                        }
                    }
                    _ => break,
                }
            }
            let status: ExitStatus = std::process::ExitStatus::from_raw(raw).into();
            self.status = Some(status.clone());
            Ok(Some(status))
        }
    }

    impl Child for PtyChild {
        fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            self.reap(libc::WNOHANG)
        }

        fn wait(&mut self) -> io::Result<ExitStatus> {
            self.reap(0)
                .map(|s| s.expect("blocking waitpid returned no status"))
        }

        fn process_id(&self) -> Option<u32> {
            Some(self.pid as u32)
        }
    }

    impl ChildKiller for PtyChild {
        fn kill(&mut self) -> io::Result<()> {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            if unsafe { libc::kill(self.pid, libc::SIGHUP) } != 0 {
                return Err(io::Error::last_os_error());
            }
            for attempt in 0..5 {
                if attempt > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                if self.try_wait()?.is_some() {
                    return Ok(());
                }
            }
            if unsafe { libc::kill(self.pid, libc::SIGKILL) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(Hangup(self.pid))
        }
    }

    /// portable-pty's cloned killer: SIGHUP only.
    #[derive(Debug)]
    struct Hangup(pid_t);

    impl ChildKiller for Hangup {
        fn kill(&mut self) -> io::Result<()> {
            if unsafe { libc::kill(self.0, libc::SIGHUP) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(Hangup(self.0))
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
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
}
