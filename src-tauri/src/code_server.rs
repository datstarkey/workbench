use std::collections::HashMap;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Context, Result};
use tauri::{AppHandle, Emitter};

use crate::paths::enriched_path;
use crate::types::{CodeServerExitEvent, CodeServerInfo};

type SessionMap = Arc<Mutex<HashMap<String, Child>>>;

pub struct CodeServerManager {
    sessions: SessionMap,
    binary_path: OnceLock<String>,
}

impl CodeServerManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            binary_path: OnceLock::new(),
        }
    }

    /// Locate the `code-server` binary on the system (cached after first call).
    pub fn detect(&self) -> Result<String> {
        if let Some(path) = self.binary_path.get() {
            return Ok(path.clone());
        }

        let output = Command::new("which")
            .arg("code-server")
            .env("PATH", enriched_path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .context("Failed to run 'which code-server'")?;

        if !output.status.success() {
            return Err(anyhow!(
                "code-server not found. Install it with: brew install code-server"
            ));
        }

        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() {
            return Err(anyhow!(
                "code-server not found. Install it with: brew install code-server"
            ));
        }

        // Cache for future calls (ignore race — both threads find the same path)
        let _ = self.binary_path.set(path.clone());
        Ok(path)
    }

    /// Find an available port by binding to port 0 and reading the OS-assigned port.
    fn find_available_port() -> Result<u16> {
        let listener =
            TcpListener::bind("127.0.0.1:0").context("Failed to bind ephemeral port")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        Ok(port)
    }

    /// Resolve the user's VS Code user-data and extensions directories.
    fn vscode_dirs() -> (Option<String>, Option<String>) {
        let home = dirs::home_dir();

        let user_data_dir = {
            #[cfg(target_os = "macos")]
            {
                home.as_ref().map(|h| {
                    h.join("Library/Application Support/Code")
                        .to_string_lossy()
                        .to_string()
                })
            }
            #[cfg(target_os = "linux")]
            {
                home.as_ref()
                    .map(|h| h.join(".config/Code").to_string_lossy().to_string())
            }
            #[cfg(target_os = "windows")]
            {
                dirs::config_dir().map(|c| c.join("Code").to_string_lossy().to_string())
            }
        };

        let extensions_dir = home
            .as_ref()
            .map(|h| h.join(".vscode/extensions").to_string_lossy().to_string());

        // Only return dirs that actually exist
        let user_data = user_data_dir.filter(|p| std::path::Path::new(p).is_dir());
        let extensions = extensions_dir.filter(|p| std::path::Path::new(p).is_dir());

        (user_data, extensions)
    }

    /// Start a code-server instance for the given session.
    pub fn start(
        &self,
        session_id: String,
        project_path: String,
        app_handle: AppHandle,
    ) -> Result<CodeServerInfo> {
        let binary = self.detect()?;
        let port = Self::find_available_port()?;

        let mut cmd = Command::new(&binary);
        cmd.arg("--bind-addr")
            .arg(format!("127.0.0.1:{}", port))
            .arg("--auth")
            .arg("none")
            .arg("--disable-telemetry")
            .arg("--disable-workspace-trust");

        let (user_data_dir, extensions_dir) = Self::vscode_dirs();
        if let Some(dir) = &user_data_dir {
            cmd.arg("--user-data-dir").arg(dir);
        }
        if let Some(dir) = &extensions_dir {
            cmd.arg("--extensions-dir").arg(dir);
        }

        cmd.arg(&project_path);

        cmd.env("PATH", enriched_path());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        let child = cmd.spawn().context(format!(
            "Failed to start code-server at '{}'",
            binary
        ))?;

        // URL-encode the project path for the query string (handles spaces, #, etc.)
        let encoded_path = project_path
            .replace('%', "%25")
            .replace(' ', "%20")
            .replace('#', "%23")
            .replace('?', "%3F")
            .replace('&', "%26");
        let url = format!("http://127.0.0.1:{}/?folder={}", port, encoded_path);

        let info = CodeServerInfo {
            session_id: session_id.clone(),
            port,
            url,
            project_path: project_path.clone(),
        };

        // Take ownership of the child's wait handle for the monitoring thread.
        // The child.kill() handle stays in the sessions map.
        // We use child.id() to track, then move child into sessions after spawning.
        //
        // To avoid the deadlock of holding the sessions lock during child.wait(),
        // we take the child OUT of the map for wait, matching pty.rs's pattern of
        // only holding the outer lock briefly.
        let sessions_for_cleanup = Arc::clone(&self.sessions);
        let sid = session_id.clone();

        // Store session — lock held only for insert
        {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            sessions.insert(session_id.clone(), child);
        }

        // Spawn monitoring thread. Takes the child OUT of the map to wait on it,
        // so the lock is never held during blocking I/O.
        std::thread::spawn(move || {
            // Brief lock: remove child from map so we can wait without holding the lock
            let mut child = {
                let mut sessions = sessions_for_cleanup
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                match sessions.remove(&sid) {
                    Some(c) => c,
                    None => return, // Already removed by stop()
                }
            };

            // Wait WITHOUT holding any lock
            let exit_code = match child.wait() {
                Ok(status) => status.code().unwrap_or(-1),
                Err(_) => -1,
            };

            let _ = app_handle.emit(
                "code-server:exit",
                CodeServerExitEvent {
                    session_id: sid,
                    exit_code,
                },
            );
        });

        Ok(info)
    }

    /// Stop a code-server instance. Idempotent — no error if already stopped.
    pub fn stop(&self, session_id: &str) -> Result<()> {
        let mut child = {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            match sessions.remove(session_id) {
                Some(c) => c,
                None => return Ok(()), // Already cleaned up
            }
        };
        // Kill outside the lock
        let _ = child.kill();
        let _ = child.wait();
        Ok(())
    }

    /// Stop all running code-server instances. Called on app exit.
    pub fn stop_all(&self) {
        let children: Vec<Child> = {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            sessions.drain().map(|(_, c)| c).collect()
        };
        // Kill outside the lock
        for mut child in children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
