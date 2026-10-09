//! One process keeps a config dir's workspace model: the desktop and a
//! standalone server on the same dir would otherwise both boot (and run) every
//! saved pane. An advisory pid file; a stale one (its process gone) is taken.

use std::io::Write;
use std::path::{Path, PathBuf};

const FILE: &str = "workspaces.v2.lock";

pub(super) struct ModelLock(PathBuf);

impl ModelLock {
    /// Take the dir's lock, or `None` while another live process holds it.
    pub(super) fn take(dir: &Path) -> Option<Self> {
        let path = dir.join(FILE);
        let me = std::process::id();
        for _ in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    let _ = write!(file, "{me}");
                    return Some(Self(path));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => match holder(&path) {
                    // A service of this process held it before (a restart in tests).
                    Some(pid) if pid == me => return Some(Self(path)),
                    Some(pid) if alive(pid) => return None,
                    _ => {
                        let _ = std::fs::remove_file(&path);
                    }
                },
                Err(e) => {
                    tracing::warn!("workspace lock {}: {e}", path.display());
                    return None;
                }
            }
        }
        None
    }
}

impl ModelLock {
    /// The process holding `dir`'s lock, if it says.
    pub(super) fn holder_of(dir: &Path) -> Option<u32> {
        holder(&dir.join(FILE))
    }
}

impl Drop for ModelLock {
    fn drop(&mut self) {
        if holder(&self.0) == Some(std::process::id()) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

fn holder(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // Signal 0 only checks: EPERM means it exists under another user.
    let found = unsafe { libc::kill(pid, 0) } == 0;
    found || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    workbench_core::shell::command("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(&pid.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_holder_keeps_it_and_a_stale_one_doesnt() {
        let dir = tempfile::tempdir().unwrap();
        let held = ModelLock::take(dir.path()).expect("free");
        assert!(ModelLock::take(dir.path()).is_some(), "this process again");
        drop(held);
        assert!(!dir.path().join(FILE).exists(), "released");

        // Process 1 is always alive and never us.
        std::fs::write(dir.path().join(FILE), "1").unwrap();
        #[cfg(unix)]
        assert!(ModelLock::take(dir.path()).is_none());
        // A pid nothing runs as is stale.
        std::fs::write(dir.path().join(FILE), "4194303999").unwrap();
        assert!(ModelLock::take(dir.path()).is_some());
    }
}
