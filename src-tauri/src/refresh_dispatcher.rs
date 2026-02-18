use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::types::ProjectRefreshRequestedEvent;

struct PendingRefresh {
    generation: u64,
    source: String,
    trigger: String,
}

pub struct RefreshDispatcher {
    pending: Arc<Mutex<HashMap<String, PendingRefresh>>>,
    debounce_window: Duration,
}

impl RefreshDispatcher {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            debounce_window: Duration::from_millis(500),
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

        let generation = self.enqueue_request(project_path.clone(), source, trigger);

        let pending = Arc::clone(&self.pending);
        let handle = app_handle.clone();
        let debounce_window = self.debounce_window;

        std::thread::spawn(move || {
            std::thread::sleep(debounce_window);

            let payload = {
                let mut pending = pending.lock().unwrap_or_else(|e| e.into_inner());
                let Some(entry) = pending.get(&project_path) else {
                    return;
                };
                if entry.generation != generation {
                    return;
                }
                let payload = ProjectRefreshRequestedEvent {
                    project_path: project_path.clone(),
                    source: entry.source.clone(),
                    trigger: entry.trigger.clone(),
                };
                pending.remove(&project_path);
                payload
            };

            let _ = handle.emit("project:refresh-requested", payload);
        });
    }

    fn enqueue_request(&self, project_path: String, source: &str, trigger: &str) -> u64 {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let entry = pending
            .entry(project_path)
            .or_insert_with(|| PendingRefresh {
                generation: 0,
                source: source.to_string(),
                trigger: trigger.to_string(),
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

        let first = dispatcher.enqueue_request(project_path.clone(), "git-watcher", "git-dir-change");
        let second = dispatcher.enqueue_request(project_path.clone(), "claude-hook", "post-tool-use-bash");

        assert_eq!(first, 1);
        assert_eq!(second, 2);

        assert!(
            dispatcher
                .take_payload_if_latest(project_path.as_str(), first)
                .is_none()
        );

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
        let generation = dispatcher.enqueue_request("/repo".to_string(), "git-watcher", "git-dir-change");

        let emitted = dispatcher
            .take_payload_if_latest("/repo", generation)
            .expect("entry should be available");
        assert_eq!(emitted.project_path, "/repo");

        assert!(
            dispatcher
                .take_payload_if_latest("/repo", generation)
                .is_none()
        );
    }
}
