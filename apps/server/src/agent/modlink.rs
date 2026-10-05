//! A chat session fed by the `workbench` plugin inside an interactive
//! `claude` in a terminal pane, instead of a `claude -p` process. The plugin
//! posts the stream-json lines `-p` would print and long-polls for the lines
//! `-p` would read, so the driver and transcript are the same as a chat's.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use base64::Engine;
use serde_json::Value;
use tokio::sync::Notify;
use workbench_core::chat_attachment::PDF_TYPE;

use super::{lock, PromptFile, PromptImage};

/// No poll for this long: the terminal's `claude` has gone.
const STALE: Duration = Duration::from_secs(45);

/// What a terminal's token lets its plugin attach as.
#[derive(Clone, Debug)]
pub struct ModGrant {
    pub pane_id: Option<String>,
    pub project_path: String,
    pub worktree_path: Option<String>,
    pub claude_account_id: Option<String>,
    pub cwd: String,
    /// The desktop's hook socket, for a restart (a rewind) to keep.
    pub hook_socket: Option<String>,
    /// Where a rewound terminal resumed from: history shows the conversation cut there.
    pub resume_at: Option<String>,
    /// The terminal the token was issued to, once created.
    pub terminal_id: Option<String>,
}

pub struct ModLink {
    /// The terminal token the plugin attached with; every request must carry it.
    pub token: String,
    /// The server terminal whose `claude` this is, when known.
    pub terminal_id: Option<String>,
    queue: Mutex<VecDeque<Value>>,
    notify: Notify,
    last_seen: Mutex<Instant>,
}

impl ModLink {
    pub fn new(token: String, terminal_id: Option<String>) -> Self {
        Self {
            token,
            terminal_id,
            queue: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
            last_seen: Mutex::new(Instant::now()),
        }
    }

    pub fn push(&self, line: Value) {
        lock(&self.queue).push_back(line);
        self.notify.notify_one();
    }

    /// The queued lines, waiting up to `wait` for the first.
    pub async fn take(&self, wait: Duration) -> Vec<Value> {
        self.touch();
        let notified = self.notify.notified();
        if lock(&self.queue).is_empty() {
            let _ = tokio::time::timeout(wait, notified).await;
        }
        self.touch();
        lock(&self.queue).drain(..).collect()
    }

    pub fn touch(&self) {
        *lock(&self.last_seen) = Instant::now();
    }

    pub fn is_stale(&self) -> bool {
        lock(&self.last_seen).elapsed() > STALE
    }
}

/// A terminal `claude` takes a prompt as text, so attachments are saved to a
/// temp folder and mentioned as `@path`, which Claude Code reads (images
/// included). Returns the prompt with the mentions appended.
pub fn attachments_as_mentions(
    session_id: &str,
    text: &str,
    images: &[PromptImage],
    files: &[PromptFile],
) -> Result<String> {
    if images.is_empty() && files.is_empty() {
        return Ok(text.to_string());
    }
    let dir = std::env::temp_dir()
        .join("workbench-chat")
        .join(session_id)
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut paths = Vec::new();
    for (i, image) in images.iter().enumerate() {
        let ext = image.media_type.rsplit('/').next().unwrap_or("png");
        let path = dir.join(format!("image-{}.{ext}", i + 1));
        std::fs::write(&path, b64.decode(&image.data).context("decode image")?)?;
        paths.push(path);
    }
    for file in files {
        let name: String = std::path::Path::new(&file.name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let path = dir.join(name);
        let bytes = if file.media_type == PDF_TYPE {
            b64.decode(&file.data).context("decode PDF")?
        } else {
            file.data.clone().into_bytes()
        };
        std::fs::write(&path, bytes)?;
        paths.push(path);
    }
    let mentions: Vec<String> = paths
        .iter()
        .map(|p| {
            let p = p.to_string_lossy();
            if p.contains(' ') {
                format!("@\"{p}\"")
            } else {
                format!("@{p}")
            }
        })
        .collect();
    Ok(format!("{text}\n\n{}", mentions.join(" "))
        .trim_start()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn take_returns_queued_lines_or_times_out_empty() {
        let link = ModLink::new("t".into(), None);
        assert!(link.take(Duration::from_millis(20)).await.is_empty());
        link.push(json!({"a": 1}));
        link.push(json!({"b": 2}));
        assert_eq!(link.take(Duration::from_secs(5)).await.len(), 2);
    }

    #[test]
    fn attachments_become_mentions_of_saved_files() {
        let image = PromptImage {
            media_type: "image/png".into(),
            data: "aGk=".into(),
        };
        let file = PromptFile {
            name: "../notes.txt".into(),
            media_type: "text/plain".into(),
            data: "hello".into(),
        };
        let text = attachments_as_mentions("s1", "look", &[image], &[file]).unwrap();
        let (prompt, mentions) = text.split_once("\n\n").unwrap();
        assert_eq!(prompt, "look");
        let paths: Vec<&str> = mentions
            .split(' ')
            .map(|m| m.trim_start_matches('@'))
            .collect();
        assert_eq!(std::fs::read(paths[0]).unwrap(), b"hi");
        assert!(
            paths[1].ends_with("notes.txt"),
            "a name can't climb out of the folder"
        );
        assert_eq!(std::fs::read_to_string(paths[1]).unwrap(), "hello");
        assert_eq!(
            attachments_as_mentions("s1", "plain", &[], &[]).unwrap(),
            "plain"
        );
    }

    #[tokio::test]
    async fn a_waiting_take_wakes_on_push() {
        let link = std::sync::Arc::new(ModLink::new("t".into(), None));
        let waiter = link.clone();
        let task = tokio::spawn(async move { waiter.take(Duration::from_secs(5)).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        link.push(json!({"type": "user"}));
        assert_eq!(task.await.unwrap(), vec![json!({"type": "user"})]);
    }
}
