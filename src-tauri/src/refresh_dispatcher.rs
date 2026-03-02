use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::types::ProjectRefreshRequestedEvent;

struct PendingRefresh {
    generation: u64,
    source: String,
    trigger: String,
    last_emitted_at: Option<Instant>,
}

struct RefreshRequest {
    app_handle: AppHandle,
    project_path: String,
}

pub struct RefreshDispatcher {
    pending: Arc<Mutex<HashMap<String, PendingRefresh>>>,
    sender: Mutex<mpsc::Sender<RefreshRequest>>,
    debounce_window: Duration,
    _worker: thread::JoinHandle<()>,
}

impl RefreshDispatcher {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel::<RefreshRequest>();
        let pending: Arc<Mutex<HashMap<String, PendingRefresh>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let worker_pending = Arc::clone(&pending);
        let debounce_window = Duration::from_millis(300);

        let worker = thread::spawn(move || {
            Self::worker_loop(receiver, worker_pending, debounce_window);
        });

        Self {
            pending,
            sender: Mutex::new(sender),
            debounce_window,
            _worker: worker,
        }
    }

    pub fn request_refresh(
        &self,
        app_handle: &AppHandle,
        project_path: String,
        source: &str,
        trigger: &str,
    ) {
        if project_path.trim().is_empty() {
            return;
        }

        let should_emit_now = {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            let entry = pending
                .entry(project_path.clone())
                .or_insert_with(|| PendingRefresh {
                    generation: 0,
                    source: source.to_string(),
                    trigger: trigger.to_string(),
                    last_emitted_at: None,
                });
            entry.generation += 1;
            entry.source = source.to_string();
            entry.trigger = trigger.to_string();

            // Leading edge: emit immediately if outside debounce window
            match entry.last_emitted_at {
                None => true,
                Some(last) => last.elapsed() >= self.debounce_window,
            }
        };

        if should_emit_now {
            self.emit_now(app_handle, &project_path);
        }

        // Always schedule a trailing-edge check via the worker
        let _ = self
            .sender
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .send(RefreshRequest {
                app_handle: app_handle.clone(),
                project_path,
            });
    }

    fn emit_now(&self, app_handle: &AppHandle, project_path: &str) {
        let payload = {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            let Some(entry) = pending.get_mut(project_path) else {
                return;
            };
            let payload = ProjectRefreshRequestedEvent {
                project_path: project_path.to_string(),
                source: entry.source.clone(),
                trigger: entry.trigger.clone(),
            };
            entry.last_emitted_at = Some(Instant::now());
            // Reset generation to 0 so trailing-edge knows this was consumed
            entry.generation = 0;
            payload
        };
        let _ = app_handle.emit("project:refresh-requested", payload);
    }

    fn worker_loop(
        receiver: mpsc::Receiver<RefreshRequest>,
        pending: Arc<Mutex<HashMap<String, PendingRefresh>>>,
        debounce_window: Duration,
    ) {
        // Collect trailing-edge timers: (deadline, project_path, generation_at_schedule, app_handle)
        let mut timers: Vec<(Instant, String, u64, AppHandle)> = Vec::new();

        loop {
            // Calculate timeout: either the nearest timer deadline or a long wait
            let timeout = timers
                .iter()
                .map(|(deadline, _, _, _)| deadline.saturating_duration_since(Instant::now()))
                .min()
                .unwrap_or(Duration::from_secs(60));

            match receiver.recv_timeout(timeout) {
                Ok(req) => {
                    // New request came in. Schedule a trailing-edge timer.
                    let (generation, remaining) = {
                        let pending = pending.lock().unwrap_or_else(|e| e.into_inner());
                        let Some(entry) = pending.get(&req.project_path) else {
                            continue;
                        };
                        let remaining = match entry.last_emitted_at {
                            Some(last) => debounce_window.saturating_sub(last.elapsed()),
                            None => debounce_window,
                        };
                        (entry.generation, remaining)
                    };
                    let deadline = Instant::now() + remaining;
                    timers.push((deadline, req.project_path, generation, req.app_handle));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // A timer may have expired, process below
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }

            // Fire any expired trailing-edge timers
            let now = Instant::now();
            let mut fired_projects: Vec<String> = Vec::new();
            timers.retain(|(deadline, project_path, gen_at_schedule, app_handle)| {
                if *deadline > now {
                    return true; // not yet
                }
                // Only fire if generation hasn't changed since scheduling
                // (meaning no leading-edge fire or newer request consumed it)
                let should_fire = {
                    let pending = pending.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(entry) = pending.get(project_path) {
                        entry.generation == *gen_at_schedule && entry.generation > 0
                    } else {
                        false
                    }
                };
                if should_fire && !fired_projects.contains(project_path) {
                    let payload = {
                        let mut pending = pending.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(entry) = pending.get_mut(project_path) {
                            entry.last_emitted_at = Some(Instant::now());
                            entry.generation = 0;
                            Some(ProjectRefreshRequestedEvent {
                                project_path: project_path.clone(),
                                source: entry.source.clone(),
                                trigger: entry.trigger.clone(),
                            })
                        } else {
                            None
                        }
                    };
                    if let Some(payload) = payload {
                        let _ = app_handle.emit("project:refresh-requested", payload);
                        fired_projects.push(project_path.clone());
                    }
                }
                false // remove expired timer
            });
        }
    }

    #[cfg(test)]
    fn enqueue_request(&self, project_path: String, source: &str, trigger: &str) -> u64 {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let entry = pending
            .entry(project_path)
            .or_insert_with(|| PendingRefresh {
                generation: 0,
                source: source.to_string(),
                trigger: trigger.to_string(),
                last_emitted_at: None,
            });
        entry.generation += 1;
        entry.source = source.to_string();
        entry.trigger = trigger.to_string();
        entry.generation
    }

    #[cfg(test)]
    fn take_payload_if_latest(
        &self,
        project_path: &str,
        generation: u64,
    ) -> Option<ProjectRefreshRequestedEvent> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let entry = pending.get(project_path)?;
        if entry.generation != generation {
            return None;
        }
        let payload = ProjectRefreshRequestedEvent {
            project_path: project_path.to_string(),
            source: entry.source.clone(),
            trigger: entry.trigger.clone(),
        };
        pending.remove(project_path);
        Some(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::RefreshDispatcher;

    #[test]
    fn enqueue_overwrites_source_and_trigger_for_latest_generation() {
        let dispatcher = RefreshDispatcher::new();
        let project_path = "/repo".to_string();

        let first =
            dispatcher.enqueue_request(project_path.clone(), "git-watcher", "git-dir-change");
        let second = dispatcher.enqueue_request(
            project_path.clone(),
            "claude-hook",
            "post-tool-use-bash",
        );

        assert_eq!(first, 1);
        assert_eq!(second, 2);

        assert!(dispatcher
            .take_payload_if_latest(project_path.as_str(), first)
            .is_none());

        let latest = dispatcher
            .take_payload_if_latest(project_path.as_str(), second)
            .expect("latest generation should emit");
        assert_eq!(latest.project_path, "/repo");
        assert_eq!(latest.source, "claude-hook");
        assert_eq!(latest.trigger, "post-tool-use-bash");
    }

    #[test]
    fn take_payload_if_latest_consumes_entry() {
        let dispatcher = RefreshDispatcher::new();
        let generation =
            dispatcher.enqueue_request("/repo".to_string(), "git-watcher", "git-dir-change");

        let emitted = dispatcher
            .take_payload_if_latest("/repo", generation)
            .expect("entry should be available");
        assert_eq!(emitted.project_path, "/repo");

        assert!(dispatcher
            .take_payload_if_latest("/repo", generation)
            .is_none());
    }
}
