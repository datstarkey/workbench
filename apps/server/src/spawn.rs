//! Spawns and tracks `claude remote-control` processes.
//!
//! Each spawned process runs inside a PTY (so the long-running `claude` session
//! stays alive and behaves as if attached to a terminal) but we never proxy its
//! IO anywhere. `claude remote-control` registers the session with Anthropic's
//! API over outbound HTTPS, so it shows up in the Claude mobile app / claude.ai
//! on its own. We only keep the handle so the session can be listed and killed,
//! and scan its early output for the printed session URL as a convenience.

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum RemoteStatus {
    Starting,
    Running,
    Exited { code: i64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSession {
    pub id: String,
    pub name: Option<String>,
    pub cwd: String,
    pub pid: Option<u32>,
    pub status: RemoteStatus,
    pub session_url: Option<String>,
    /// Unix epoch milliseconds.
    pub started_at: u64,
}

struct Tracked {
    meta: RemoteSession,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    // Master is kept alive so the PTY (and thus the child) is not torn down.
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

/// Backstop against runaway spawns (the server is unauthenticated by default, so
/// this caps process/PTY/thread creation even if the network boundary is bypassed).
const MAX_REMOTE_SESSIONS: usize = 64;

#[derive(Clone, Default)]
pub struct RemoteControlManager {
    inner: Arc<Mutex<HashMap<String, Tracked>>>,
}

impl RemoteControlManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve the working directory for a spawn request, restricted to directories
    /// Workbench manages. A worktree path must be a known worktree of the project
    /// (reuses core's worktree listing); a bare project path must be a **registered
    /// Workbench project** (`registered_projects`). This stops an unauthenticated
    /// caller from running a session/shell in an arbitrary directory on the host.
    pub(crate) fn resolve_cwd(
        project_path: &str,
        worktree_path: Option<&str>,
        registered_projects: &[String],
    ) -> Result<String> {
        match worktree_path {
            Some(wt) => {
                let worktrees = workbench_core::git::list_worktrees(project_path)
                    .context("failed to list worktrees")?;
                let known = worktrees.iter().any(|w| w.path == wt);
                if !known {
                    bail!("worktree path is not a known worktree of this project: {wt}");
                }
                Ok(wt.to_string())
            }
            None => {
                if !std::path::Path::new(project_path).is_dir() {
                    bail!("project path does not exist: {project_path}");
                }
                if !registered_projects.iter().any(|p| p == project_path) {
                    bail!("project path is not a registered Workbench project: {project_path}");
                }
                Ok(project_path.to_string())
            }
        }
    }

    pub fn spawn(
        &self,
        project_path: &str,
        worktree_path: Option<&str>,
        name: Option<String>,
        registered_projects: &[String],
    ) -> Result<RemoteSession> {
        let cwd = Self::resolve_cwd(project_path, worktree_path, registered_projects)?;

        {
            let map = self
                .inner
                .lock()
                .map_err(|_| anyhow::anyhow!("spawn manager lock poisoned"))?;
            if map.len() >= MAX_REMOTE_SESSIONS {
                bail!("remote session limit reached ({MAX_REMOTE_SESSIONS})");
            }
        }

        let id = uuid::Uuid::new_v4().to_string();

        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: 40,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to open PTY")?;

        // Binary is overridable for tests (point at a fake script) and to support
        // alternative CLIs later.
        let bin = std::env::var("WORKBENCH_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());
        let mut cmd = CommandBuilder::new(&bin);
        cmd.arg("remote-control");
        if let Some(ref n) = name {
            cmd.arg("--name");
            cmd.arg(n);
        }
        cmd.cwd(&cwd);
        // Inherit a usable environment so `claude` resolves on PATH and finds the
        // user's credentials/home.
        for key in ["PATH", "HOME", "USER", "LANG", "SHELL", "LOGNAME"] {
            if let Ok(val) = std::env::var(key) {
                cmd.env(key, val);
            }
        }
        cmd.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("failed to spawn `claude remote-control` (is the Claude CLI installed?)")?;
        drop(pair.slave);

        let pid = child.process_id();
        let started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let meta = RemoteSession {
            id: id.clone(),
            name,
            cwd,
            pid,
            status: RemoteStatus::Starting,
            session_url: None,
            started_at,
        };

        // Clone the reader BEFORE moving the master into the map, then insert the
        // tracked entry FIRST so the reader thread can never update a session that
        // isn't in the map yet (the child can print its URL almost immediately).
        let reader = pair.master.try_clone_reader().ok();

        let tracked = Tracked {
            meta: meta.clone(),
            child,
            _master: pair.master,
        };
        self.inner
            .lock()
            .map_err(|_| anyhow::anyhow!("spawn manager lock poisoned"))?
            .insert(id.clone(), tracked);

        // Scan early output for the session URL, then keep draining so the PTY
        // buffer never fills and blocks the child. On EOF (child gone) remove the
        // entry so self-exited sessions don't accumulate.
        if let Some(mut reader) = reader {
            let inner = self.inner.clone();
            let sid = id;
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let mut accumulated = String::new();
                let mut found = false;
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if !found {
                                accumulated.push_str(&String::from_utf8_lossy(&buf[..n]));
                                if let Some(url) = extract_url(&accumulated) {
                                    found = true;
                                    if let Ok(mut map) = inner.lock() {
                                        if let Some(t) = map.get_mut(&sid) {
                                            t.meta.session_url = Some(url);
                                            t.meta.status = RemoteStatus::Running;
                                        }
                                    }
                                }
                                // Bound memory if no URL ever appears.
                                if accumulated.len() > 64 * 1024 {
                                    found = true;
                                }
                            }
                        }
                    }
                }
                // Child closed its PTY → it has exited; stop tracking it.
                if let Ok(mut map) = inner.lock() {
                    map.remove(&sid);
                }
            });
        }

        Ok(meta)
    }

    pub fn list(&self) -> Vec<RemoteSession> {
        let mut map = match self.inner.lock() {
            Ok(m) => m,
            Err(p) => p.into_inner(),
        };
        // Reap exited children so status reflects reality.
        for tracked in map.values_mut() {
            if let Ok(Some(status)) = tracked.child.try_wait() {
                tracked.meta.status = RemoteStatus::Exited {
                    code: status.exit_code() as i64,
                };
            }
        }
        map.values().map(|t| t.meta.clone()).collect()
    }

    /// Kill and stop tracking a session. Returns `false` if no such session
    /// exists (already killed, or self-exited and reaped by the reader thread) so
    /// the route can stay idempotent rather than 500ing on a normal race.
    pub fn kill(&self, id: &str) -> bool {
        let mut map = match self.inner.lock() {
            Ok(m) => m,
            Err(p) => p.into_inner(),
        };
        match map.remove(id) {
            Some(mut tracked) => {
                let _ = tracked.child.kill();
                true
            }
            None => false,
        }
    }
}

/// Pull the session URL out of accumulated terminal output. `claude
/// remote-control` prints a docs link in its banner alongside the real session
/// URL, so we collect every http(s) URL and prefer one that isn't documentation.
fn extract_url(text: &str) -> Option<String> {
    let mut urls = Vec::new();
    let mut rest = text;
    while let Some(rel) = rest.find("https://").or_else(|| rest.find("http://")) {
        let tail = &rest[rel..];
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '\u{1b}' || c == '"' || c == '\'')
            .unwrap_or(tail.len());
        let url = tail[..end].trim_end_matches(['.', ',', ')']);
        if url.len() > "https://".len() {
            urls.push(url.to_string());
        }
        rest = &tail[end.max(1)..];
    }
    urls.iter()
        .find(|u| !u.contains("/docs/"))
        .or_else(|| urls.first())
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_session_url() {
        let out = "Starting session...\nOpen https://claude.ai/code/abc-123 to connect\n";
        assert_eq!(
            extract_url(out).as_deref(),
            Some("https://claude.ai/code/abc-123")
        );
    }

    #[test]
    fn no_url_returns_none() {
        assert_eq!(extract_url("just some text"), None);
    }

    #[test]
    fn resolve_cwd_accepts_registered_project_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let registered = vec![path.to_string()];
        let resolved = RemoteControlManager::resolve_cwd(path, None, &registered).unwrap();
        assert_eq!(resolved, path);
    }

    #[test]
    fn resolve_cwd_rejects_unregistered_existing_dir() {
        // A real directory that is not a registered project must be rejected so a
        // caller can't spawn in an arbitrary host directory.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert!(RemoteControlManager::resolve_cwd(path, None, &[]).is_err());
    }

    #[test]
    fn resolve_cwd_rejects_missing_project_dir() {
        assert!(RemoteControlManager::resolve_cwd("/no/such/dir", None, &[]).is_err());
    }

    #[test]
    fn resolve_cwd_rejects_unknown_worktree() {
        let dir = tempfile::tempdir().unwrap();
        // A worktree path that isn't a known worktree of the (non-repo) project.
        let res = RemoteControlManager::resolve_cwd(
            dir.path().to_str().unwrap(),
            Some("/tmp/elsewhere"),
            &[],
        );
        assert!(res.is_err());
    }

    #[test]
    fn strips_trailing_punctuation_and_ansi() {
        let out = "see https://claude.ai/code/x.\u{1b}[0m";
        assert_eq!(
            extract_url(out).as_deref(),
            Some("https://claude.ai/code/x")
        );
    }

    #[test]
    fn prefers_session_url_over_docs_link() {
        let out = "Docs: https://code.claude.com/docs/en/remote-control\nSession: https://claude.ai/code/sess-42\n";
        assert_eq!(
            extract_url(out).as_deref(),
            Some("https://claude.ai/code/sess-42")
        );
    }
}
