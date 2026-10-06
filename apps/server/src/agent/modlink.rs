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
use workbench_core::claude_transcript::WaitingSummary;

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
    /// The mode picked in chat it was started in, kept across a restart.
    pub permission_mode: Option<String>,
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
    /// Approvals the plugin waits on (`/mod/ask`), by request id, with the
    /// answer once a client gives it.
    asks: Mutex<std::collections::HashMap<String, Option<Value>>>,
    answered: Notify,
    /// The tool call each approval in `asks` is for.
    ask_tools: Mutex<std::collections::HashMap<String, Option<String>>>,
    /// What the terminal's own dialogs ask (no chat was open), oldest first,
    /// each with its tool call: what the session waits on until the plugin says
    /// it was answered, the call has a result, or the turn ends.
    in_terminal: Mutex<Vec<(WaitingSummary, Option<String>)>>,
}

/// A line only the server reads: the terminal shows a dialog the plugin can't
/// route through `/mod/ask` (an MCP elicitation), named by the TUI's
/// Notification hook. It isn't folded into the transcript.
const TERMINAL_WAITING: &str = "workbench_terminal_waiting";

impl ModLink {
    pub fn new(token: String, terminal_id: Option<String>) -> Self {
        Self {
            token,
            terminal_id,
            queue: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
            last_seen: Mutex::new(Instant::now()),
            asks: Mutex::new(std::collections::HashMap::new()),
            answered: Notify::new(),
            ask_tools: Mutex::new(std::collections::HashMap::new()),
            in_terminal: Mutex::new(Vec::new()),
        }
    }

    /// The plugin asked for approval `request_id` (for tool call `tool_use_id`):
    /// answers go to `/mod/ask`, not `/mod/in`.
    pub fn expect_answer(&self, request_id: &str, tool_use_id: Option<String>) {
        lock(&self.asks)
            .entry(request_id.to_string())
            .or_insert(None);
        lock(&self.ask_tools).insert(request_id.to_string(), tool_use_id);
    }

    /// Take a client's answer to an approval the plugin waits on; `false` when
    /// nobody waits on it (it goes to `/mod/in` like any line).
    pub fn answer(&self, line: &Value) -> bool {
        let id = line.pointer("/response/request_id").and_then(Value::as_str);
        let mut asks = lock(&self.asks);
        match id.and_then(|id| asks.get_mut(id)) {
            Some(slot) => {
                *slot = Some(line.clone());
                self.answered.notify_waiters();
                true
            }
            None => false,
        }
    }

    /// Wait up to `wait` for the answer to `request_id`; `None` when there is none yet.
    pub async fn wait_answer(&self, request_id: &str, wait: Duration) -> Option<Value> {
        self.touch();
        let answered = self.answered.notified();
        if let Some(answer) = self.take_answer(request_id) {
            return Some(answer);
        }
        let _ = tokio::time::timeout(wait, answered).await;
        self.touch();
        self.take_answer(request_id)
    }

    fn take_answer(&self, request_id: &str) -> Option<Value> {
        let mut asks = lock(&self.asks);
        let answer = asks.get_mut(request_id)?.take()?;
        asks.remove(request_id);
        lock(&self.ask_tools).remove(request_id);
        Some(answer)
    }

    /// Stop waiting on `request_id`: the terminal asks it instead, and
    /// `waiting` (its summary as the chat had it) stays what the session
    /// waits on, so a phone or desktop not looking still hears of it.
    pub fn fall_back(&self, request_id: &str, waiting: Option<WaitingSummary>) {
        lock(&self.asks).remove(request_id);
        let tool = lock(&self.ask_tools).remove(request_id).flatten();
        if let Some(waiting) = waiting {
            let waiting = WaitingSummary {
                in_terminal: true,
                ..waiting
            };
            lock(&self.in_terminal).push((waiting, tool));
        }
    }

    /// The oldest dialog the terminal waits on, if any.
    pub fn terminal_waiting(&self) -> Option<WaitingSummary> {
        lock(&self.in_terminal).first().map(|(w, _)| w.clone())
    }

    /// A line the plugin posted. A terminal dialog was answered once the
    /// plugin cancels its request (the approved call starts), its tool call
    /// has a result (it ran, or was denied), or the turn ends. False for a
    /// line only the server reads, which the transcript must not see.
    pub fn note_line(&self, line: &Value) -> bool {
        let mut asked = lock(&self.in_terminal);
        let str_at = |p: &str| line.pointer(p).and_then(Value::as_str);
        match str_at("/type") {
            Some(TERMINAL_WAITING) => {
                asked.push((
                    WaitingSummary {
                        id: str_at("/id").unwrap_or_default().to_string(),
                        tool: str_at("/tool").unwrap_or("Elicitation").to_string(),
                        preview: str_at("/preview").unwrap_or_default().to_string(),
                        in_terminal: true,
                    },
                    None,
                ));
                return false;
            }
            Some("result") => asked.clear(),
            Some("control_cancel_request") => {
                asked.retain(|(w, _)| Some(w.id.as_str()) != str_at("/request_id"));
            }
            Some("user") => {
                let results: Vec<&str> = line
                    .pointer("/message/content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|b| b.get("tool_use_id").and_then(Value::as_str))
                    .collect();
                if !results.is_empty() {
                    asked
                        .retain(|(_, tool)| tool.as_deref().is_some_and(|t| !results.contains(&t)));
                }
            }
            _ => {}
        }
        true
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

/// Where a session's attachments are saved; removed when the session ends.
pub fn attachment_dir(session_id: &str) -> std::path::PathBuf {
    std::env::temp_dir().join("workbench-chat").join(session_id)
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
    let dir = attachment_dir(session_id).join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    // Other local users can read a world-readable temp dir.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for d in [dir.parent().unwrap_or(&dir), &dir] {
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut paths = Vec::new();
    for (i, image) in images.iter().enumerate() {
        let ext = image.media_type.rsplit('/').next().unwrap_or("png");
        let path = dir.join(format!("image-{}.{ext}", i + 1));
        std::fs::write(&path, b64.decode(&image.data).context("decode image")?)?;
        paths.push(path);
    }
    for (i, file) in files.iter().enumerate() {
        let name: String = std::path::Path::new(&file.name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let path = dir.join(format!("{}-{name}", i + 1));
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

    fn waiting(id: &str) -> WaitingSummary {
        WaitingSummary {
            id: id.into(),
            tool: "Bash".into(),
            preview: "ls".into(),
            in_terminal: false,
        }
    }

    #[test]
    fn a_terminal_asked_approval_waits_until_its_call_has_a_result() {
        let link = ModLink::new("t".into(), None);
        link.expect_answer("r1", Some("toolu_1".into()));
        link.fall_back("r1", Some(waiting("r1")));
        assert!(link.terminal_waiting().unwrap().in_terminal);
        let result = |id: &str| json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": id}]}});
        link.note_line(&json!({"type": "stream_event"}));
        link.note_line(&result("toolu_other"));
        assert!(link.terminal_waiting().is_some(), "another call's result");
        link.note_line(&result("toolu_1"));
        assert!(link.terminal_waiting().is_none());

        link.expect_answer("r2", None);
        link.fall_back("r2", Some(waiting("r2")));
        link.note_line(&json!({"type": "result", "subtype": "success"}));
        assert!(link.terminal_waiting().is_none(), "the turn ended");
    }

    #[test]
    fn parallel_terminal_approvals_clear_one_at_a_time() {
        let link = ModLink::new("t".into(), None);
        for (r, t) in [("r1", "toolu_1"), ("r2", "toolu_2")] {
            link.expect_answer(r, Some(t.into()));
            link.fall_back(r, Some(waiting(r)));
        }
        assert_eq!(link.terminal_waiting().unwrap().id, "r1");
        // The plugin cancels r1 as its approved call starts: a long tool isn't "waiting".
        assert!(link.note_line(&json!({"type": "control_cancel_request", "request_id": "r1"})));
        assert_eq!(link.terminal_waiting().unwrap().id, "r2");
        link.note_line(
            &json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_2"}]}}),
        );
        assert!(link.terminal_waiting().is_none());
    }

    #[test]
    fn a_terminal_elicitation_waits_and_stays_out_of_the_transcript() {
        let link = ModLink::new("t".into(), None);
        let line = json!({"type": TERMINAL_WAITING, "id": "e1", "tool": "Elicitation", "preview": "Pick one"});
        assert!(!link.note_line(&line), "not for the transcript");
        let w = link.terminal_waiting().unwrap();
        assert_eq!(
            (w.id.as_str(), w.tool.as_str(), w.in_terminal),
            ("e1", "Elicitation", true)
        );
        link.note_line(
            &json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_9"}]}}),
        );
        assert!(link.terminal_waiting().is_none(), "the MCP call returned");
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
