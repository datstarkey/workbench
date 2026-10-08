//! A chat session fed by the `workbench` plugin inside an interactive
//! `claude` in a terminal pane. The plugin posts the SDK's stream-json lines
//! for what the session does and long-polls for the lines a host sends it
//! (prompts, answers, control requests), which the Claude driver folds.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::Notify;
use workbench_core::claude_transcript::{
    WaitingSummary, TERMINAL_ELICITATION, TERMINAL_ELICITATION_ANSWERED,
};

use super::lock;

/// No poll for this long: the terminal's `claude` has gone.
const STALE: Duration = Duration::from_secs(45);
/// What may wait for a plugin that isn't polling. Attachments are files, so a
/// prompt is its text: these only bound a stuck link, not a normal turn.
const QUEUE_LINES: usize = 256;
const QUEUE_BYTES: usize = 16 * 1024 * 1024;

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
    /// For a terminal opened for a session: that id and each a `/clear` or
    /// `/resume` moved it to, the only ids it may attach as. Empty for a
    /// plain shell, which runs whatever `claude` is typed into it.
    pub session_ids: Vec<String>,
}

pub struct ModLink {
    /// The terminal token the plugin attached with; every request must carry it.
    pub token: String,
    /// The server terminal whose `claude` this is, when known.
    pub terminal_id: Option<String>,
    queue: Mutex<Inbox>,
    notify: Notify,
    /// The plugin worker's numbering (`epoch`, from its hello) and the newest
    /// line it posted that was folded (`/mod/out`, `/mod/ask`), held while a
    /// batch folds: a repeat is skipped.
    folded: Mutex<Folded>,
    last_seen: Mutex<Instant>,
    /// Approvals the plugin waits on (`/mod/ask`), by request id.
    asks: Mutex<HashMap<String, Ask>>,
    answered: Notify,
    /// What the terminal's own dialogs ask, oldest first: what the session
    /// waits on until the plugin says it was answered or the turn ends.
    in_terminal: Mutex<Vec<(WaitingSummary, Clears)>>,
}

/// Lines for the plugin (`/mod/in`), numbered, with their size. A poll
/// acknowledging none (an older plugin) takes them; otherwise each stays until
/// a later poll acknowledges it, so a reply the plugin's fetch lost is
/// delivered again.
#[derive(Default)]
struct Inbox {
    lines: VecDeque<(u64, Value, usize)>,
    bytes: usize,
    last: u64,
    /// The newest number a poll was answered with.
    delivered: u64,
    /// The newest number a poll acknowledged.
    acked: u64,
}

impl Inbox {
    fn pop_front(&mut self) {
        if let Some((_, _, bytes)) = self.lines.pop_front() {
            self.bytes -= bytes;
        }
    }
}

#[derive(Default)]
struct Folded {
    epoch: Option<String>,
    last: u64,
}

struct Ask {
    /// The tool call it is for.
    tool_use_id: Option<String>,
    /// A client's answer, once given.
    answer: Option<Value>,
    /// A chat had it open at some point.
    shown: bool,
    /// The terminal asks it instead; kept so a retry after a lost reply
    /// hears `fallback` again rather than putting the card back.
    fell_back: bool,
}

/// What else ends a wait on the terminal's dialog.
enum Clears {
    /// An approval: its tool call's result (any call's, if it named none).
    ToolResult(Option<String>),
    /// An MCP elicitation: only the plugin's word it was answered.
    Answered,
}

impl ModLink {
    pub fn new(token: String, terminal_id: Option<String>) -> Self {
        Self {
            token,
            terminal_id,
            queue: Mutex::default(),
            notify: Notify::new(),
            folded: Mutex::new(Folded::default()),
            last_seen: Mutex::new(Instant::now()),
            asks: Mutex::new(HashMap::new()),
            answered: Notify::new(),
            in_terminal: Mutex::new(Vec::new()),
        }
    }

    /// The plugin asked for approval `request_id` (for tool call `tool_use_id`):
    /// answers go to `/mod/ask`, not `/mod/in`. `false` when it already waits:
    /// the plugin resends the request until a reply shows the server has it.
    pub fn expect_answer(&self, request_id: &str, tool_use_id: Option<String>) -> bool {
        let mut asks = lock(&self.asks);
        if asks.contains_key(request_id) {
            return false;
        }
        asks.insert(
            request_id.to_string(),
            Ask {
                tool_use_id,
                answer: None,
                shown: false,
                fell_back: false,
            },
        );
        true
    }

    /// Whether a chat has shown `request_id`, counting now when `viewing`.
    /// Once one has, it stays a chat's to answer however long nobody looks:
    /// a phone drops its socket whenever the app is backgrounded.
    pub fn shown(&self, request_id: &str, viewing: bool) -> bool {
        lock(&self.asks).get_mut(request_id).map_or(viewing, |ask| {
            ask.shown |= viewing;
            ask.shown
        })
    }

    /// Take a client's answer to an approval the plugin waits on; `false` when
    /// nobody waits on it (it goes to `/mod/in` like any line).
    pub fn answer(&self, line: &Value) -> bool {
        let id = line.pointer("/response/request_id").and_then(Value::as_str);
        let mut asks = lock(&self.asks);
        match id
            .and_then(|id| asks.get_mut(id))
            .filter(|ask| !ask.fell_back)
        {
            Some(ask) => {
                ask.answer = Some(line.clone());
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

    /// Kept until its tool call's result or the turn's end: the plugin's fetch
    /// can lose a reply, and its retry must find the answer again.
    fn take_answer(&self, request_id: &str) -> Option<Value> {
        lock(&self.asks).get(request_id)?.answer.clone()
    }

    /// Stop waiting on `request_id`: the terminal asks it instead, and
    /// `waiting` (its summary as the chat had it) stays what the session
    /// waits on, so a phone or desktop not looking still hears of it.
    pub fn fall_back(&self, request_id: &str, waiting: Option<WaitingSummary>) {
        let tool = lock(&self.asks).get_mut(request_id).and_then(|a| {
            a.fell_back = true;
            a.tool_use_id.clone()
        });
        if let Some(waiting) = waiting {
            let waiting = WaitingSummary {
                in_terminal: true,
                ..waiting
            };
            lock(&self.in_terminal).push((waiting, Clears::ToolResult(tool)));
        }
    }

    /// Drop unanswered asks the chat no longer shows: a hook that died
    /// waiting would otherwise leave one open for good.
    pub fn keep_asks(&self, pending: &[String]) {
        lock(&self.asks).retain(|id, ask| ask.fell_back || pending.contains(id));
    }

    /// Whether `request_id` went to the terminal's dialog.
    pub fn fell_back(&self, request_id: &str) -> bool {
        lock(&self.asks)
            .get(request_id)
            .is_some_and(|a| a.fell_back)
    }

    /// The oldest dialog the terminal waits on, if any.
    pub fn terminal_waiting(&self) -> Option<WaitingSummary> {
        lock(&self.in_terminal).first().map(|(w, _)| w.clone())
    }

    /// A line the plugin posted. A terminal dialog was answered once the
    /// plugin cancels its request (the approved call starts) or says the
    /// elicitation was answered, an approval's tool call has a result (it
    /// ran, or was denied), or the turn ends.
    pub fn note_line(&self, line: &Value) {
        let mut asked = lock(&self.in_terminal);
        let str_at = |p: &str| line.pointer(p).and_then(Value::as_str);
        match str_at("/type") {
            Some(TERMINAL_ELICITATION) => {
                let server = str_at("/mcp_server_name").unwrap_or("MCP server");
                let message = str_at("/message").unwrap_or_default();
                asked.push((
                    WaitingSummary {
                        id: str_at("/id").unwrap_or_default().to_string(),
                        tool: "Elicitation".to_string(),
                        preview: format!("{server}: {message}"),
                        in_terminal: true,
                    },
                    Clears::Answered,
                ));
            }
            Some(TERMINAL_ELICITATION_ANSWERED) => {
                asked.retain(|(w, _)| Some(w.id.as_str()) != str_at("/id"));
            }
            // Unanswered asks the turn's end withdrew go once the transcript
            // has seen it (`keep_asks`): a background agent's may outlive it.
            Some("result") => {
                asked.clear();
                lock(&self.asks).retain(|_, ask| ask.answer.is_none() && !ask.fell_back);
            }
            Some("control_cancel_request") => {
                let id = str_at("/request_id");
                asked.retain(|(w, _)| Some(w.id.as_str()) != id);
                // Withdrawn (Esc): the plugin no longer waits on it.
                if let Some(id) = id {
                    lock(&self.asks).remove(id);
                }
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
                    lock(&self.asks).retain(|_, ask| {
                        !ask.tool_use_id
                            .as_deref()
                            .is_some_and(|t| results.contains(&t))
                    });
                    asked.retain(|(_, clears)| match clears {
                        Clears::ToolResult(Some(t)) => !results.contains(&t.as_str()),
                        Clears::ToolResult(None) => false,
                        Clears::Answered => true,
                    });
                }
            }
            _ => {}
        }
    }

    /// Queue a line for the plugin's next poll; refused while the queue is
    /// full, which only happens while the plugin isn't polling (a stuck or
    /// gone `claude`, until its link goes stale).
    pub fn push(&self, line: Value) -> anyhow::Result<()> {
        let bytes = line.to_string().len();
        let mut queue = lock(&self.queue);
        // One line of any size gets through: a long paste isn't a stuck link.
        let full = queue.lines.len() >= QUEUE_LINES || queue.bytes + bytes > QUEUE_BYTES;
        if full && !queue.lines.is_empty() {
            anyhow::bail!(
                "Claude isn't reading chat input ({} messages wait in its terminal). Check the terminal, or restart the chat.",
                queue.lines.len()
            );
        }
        queue.bytes += bytes;
        queue.last += 1;
        let seq = queue.last;
        queue.lines.push_back((seq, line, bytes));
        drop(queue);
        self.notify.notify_one();
        Ok(())
    }

    /// The lines not yet acknowledged, waiting up to `wait` for the first.
    /// `ack`: the newest the plugin handled (each line then carries its number
    /// as `wbSeq`); `None` takes them, as an older plugin expects.
    pub async fn take(&self, wait: Duration, ack: Option<u64>) -> Vec<Value> {
        self.touch();
        let notified = self.notify.notified();
        let empty = {
            let mut inbox = lock(&self.queue);
            if let Some(ack) = ack {
                // Never past what was delivered: a stale number can't drop a new line.
                let ack = ack.min(inbox.delivered);
                inbox.acked = inbox.acked.max(ack);
                while inbox.lines.front().is_some_and(|(seq, _, _)| *seq <= ack) {
                    inbox.pop_front();
                }
            }
            inbox.lines.is_empty()
        };
        if empty {
            let _ = tokio::time::timeout(wait, notified).await;
        }
        self.touch();
        let mut inbox = lock(&self.queue);
        if ack.is_none() {
            inbox.bytes = 0;
            return inbox.lines.drain(..).map(|(_, line, _)| line).collect();
        }
        inbox.delivered = inbox
            .lines
            .back()
            .map_or(inbox.delivered, |(seq, _, _)| *seq);
        inbox
            .lines
            .iter()
            .map(|(seq, line, _)| {
                let mut line = line.clone();
                if let Some(fields) = line.as_object_mut() {
                    fields.insert("wbSeq".into(), (*seq).into());
                }
                line
            })
            .collect()
    }

    /// The newest `/mod/in` line a poll acknowledged: a plugin attaching again
    /// counts from there. A line delivered to a worker that went before it
    /// acknowledged it comes again: one it never ran is worse than a repeat.
    pub fn acked(&self) -> u64 {
        lock(&self.queue).acked
    }

    /// A plugin worker said hello: one with another `epoch` (it restarted)
    /// numbers its lines from 1 again.
    pub fn hello_from(&self, epoch: Option<&str>) {
        let mut folded = lock(&self.folded);
        if epoch.is_some() && folded.epoch.as_deref() != epoch {
            *folded = Folded {
                epoch: epoch.map(String::from),
                last: 0,
            };
        }
    }

    /// Fold the plugin's numbered `lines` (the first is `seq`) with `fold`,
    /// skipping any already folded; one batch at a time. A worker's that has
    /// been replaced (its slow post landing late) are dropped. Unnumbered (an
    /// older plugin, or a line sent past the queue) all fold.
    pub fn fold_new(
        &self,
        epoch: Option<&str>,
        seq: Option<u64>,
        lines: &[Value],
        mut fold: impl FnMut(&Value),
    ) {
        let Some(first) = seq.filter(|&s| s > 0) else {
            lines.iter().for_each(fold);
            return;
        };
        let mut folded = lock(&self.folded);
        if epoch.is_some() && folded.epoch.is_some() && folded.epoch.as_deref() != epoch {
            return;
        }
        for (i, line) in lines.iter().enumerate() {
            let n = first + i as u64;
            if n > folded.last {
                fold(line);
                folded.last = n;
            }
        }
    }

    /// Whether a request carries this link's token (a constant-time compare).
    pub fn has_token(&self, token: &str) -> bool {
        workbench_core::token::constant_time_eq(self.token.as_bytes(), token.as_bytes())
    }

    pub fn touch(&self) {
        *lock(&self.last_seen) = Instant::now();
    }

    pub fn is_stale(&self) -> bool {
        lock(&self.last_seen).elapsed() > STALE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn take_returns_queued_lines_or_times_out_empty() {
        let link = ModLink::new("t".into(), None);
        assert!(link.take(Duration::from_millis(20), None).await.is_empty());
        link.push(json!({"a": 1})).unwrap();
        link.push(json!({"b": 2})).unwrap();
        assert_eq!(link.take(Duration::from_secs(5), None).await.len(), 2);
        assert!(link.take(Duration::from_millis(20), None).await.is_empty());
    }

    #[tokio::test]
    async fn a_link_nobody_polls_refuses_lines_once_full_until_a_poll_drains_it() {
        let link = ModLink::new("t".into(), None);
        let big = json!({"text": "x".repeat(QUEUE_BYTES + 1)});
        link.push(big.clone())
            .expect("one line of any size gets through");
        let err = link.push(json!({"a": 1})).unwrap_err().to_string();
        assert!(err.contains("isn't reading chat input"), "{err}");
        assert_eq!(link.take(Duration::from_millis(20), None).await, vec![big]);

        for i in 0..QUEUE_LINES {
            link.push(json!({ "i": i })).unwrap();
        }
        assert!(link.push(json!({"over": true})).is_err(), "too many lines");
        assert_eq!(
            link.take(Duration::from_millis(20), None).await.len(),
            QUEUE_LINES
        );
        link.push(json!({"after": true}))
            .expect("drained: room again");
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
    fn an_ask_a_chat_has_shown_stays_shown_after_it_closes() {
        let link = ModLink::new("t".into(), None);
        link.expect_answer("r1", None);
        assert!(!link.shown("r1", false), "nobody has looked yet");
        assert!(link.shown("r1", true));
        assert!(link.shown("r1", false), "the phone backgrounded");
        assert!(
            !link.shown("gone", false),
            "not waited on: only while viewed"
        );

        link.note_line(&json!({"type": "control_cancel_request", "request_id": "r1"}));
        assert!(!link.shown("r1", false), "withdrawn: no longer held");
    }

    #[tokio::test]
    async fn an_answer_survives_a_lost_reply_until_its_call_has_a_result() {
        let link = ModLink::new("t".into(), None);
        let wait = Duration::from_millis(20);
        link.expect_answer("r1", Some("toolu_1".into()));
        let answer = json!({"type": "control_response", "response": {"request_id": "r1"}});
        assert!(link.answer(&answer));
        assert_eq!(link.wait_answer("r1", wait).await, Some(answer.clone()));
        assert_eq!(
            link.wait_answer("r1", wait).await,
            Some(answer),
            "the plugin retries a reply its fetch lost"
        );
        link.note_line(
            &json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_1"}]}}),
        );
        assert_eq!(link.wait_answer("r1", wait).await, None);
    }

    #[test]
    fn an_unanswered_ask_goes_with_its_calls_result_or_the_turn() {
        let link = ModLink::new("t".into(), None);
        link.expect_answer("r1", Some("toolu_1".into()));
        link.expect_answer("r2", Some("toolu_2".into()));
        assert!(link.shown("r1", true) && link.shown("r2", true));
        link.note_line(
            &json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_1"}]}}),
        );
        assert!(!link.shown("r1", false), "its asking hook is gone");
        assert!(link.shown("r2", false));
        link.note_line(&json!({"type": "result", "subtype": "success"}));
        link.keep_asks(&[]);
        assert!(!link.shown("r2", false), "the turn ended and withdrew it");
        let answer = json!({"type": "control_response", "response": {"request_id": "r2"}});
        assert!(!link.answer(&answer), "nobody waits on it");
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
    fn a_fallen_back_ask_stays_the_terminals_until_its_call_has_a_result() {
        let link = ModLink::new("t".into(), None);
        link.expect_answer("r1", Some("toolu_1".into()));
        link.fall_back("r1", Some(waiting("r1")));
        assert!(link.fell_back("r1"));
        assert!(
            !link.expect_answer("r1", Some("toolu_1".into())),
            "a retry after a lost fallback reply is no new card"
        );
        let answer = json!({"type": "control_response", "response": {"request_id": "r1"}});
        assert!(!link.answer(&answer), "the terminal answers it, not a chat");
        link.note_line(
            &json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_1"}]}}),
        );
        assert!(!link.fell_back("r1"));
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
        link.note_line(&json!({"type": "control_cancel_request", "request_id": "r1"}));
        assert_eq!(link.terminal_waiting().unwrap().id, "r2");
        link.note_line(
            &json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_2"}]}}),
        );
        assert!(link.terminal_waiting().is_none());
    }

    #[test]
    fn a_terminal_elicitation_waits_until_the_terminal_answers_it() {
        let link = ModLink::new("t".into(), None);
        link.note_line(&json!({"type": TERMINAL_ELICITATION, "id": "e1",
            "mcp_server_name": "deploy", "message": "Pick one"}));
        let w = link.terminal_waiting().unwrap();
        assert_eq!(
            (
                w.id.as_str(),
                w.tool.as_str(),
                w.preview.as_str(),
                w.in_terminal
            ),
            ("e1", "Elicitation", "deploy: Pick one", true)
        );
        link.note_line(
            &json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_9"}]}}),
        );
        assert!(link.terminal_waiting().is_some(), "another call's result");
        link.note_line(
            &json!({"type": TERMINAL_ELICITATION_ANSWERED, "id": "e1", "action": "accept"}),
        );
        assert!(link.terminal_waiting().is_none());
    }

    #[tokio::test]
    async fn a_waiting_take_wakes_on_push() {
        let link = std::sync::Arc::new(ModLink::new("t".into(), None));
        let waiter = link.clone();
        let task = tokio::spawn(async move { waiter.take(Duration::from_secs(5), None).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        link.push(json!({"type": "user"})).unwrap();
        assert_eq!(task.await.unwrap(), vec![json!({"type": "user"})]);
    }

    #[tokio::test]
    async fn polled_lines_stay_until_acknowledged() {
        let link = ModLink::new("t".into(), None);
        let wait = Duration::from_millis(20);
        link.push(json!({"type": "user", "n": 1})).unwrap();
        link.push(json!({"type": "user", "n": 2})).unwrap();
        let first = link.take(wait, Some(0)).await;
        assert_eq!(first.len(), 2);
        assert_eq!(
            (first[0]["wbSeq"].as_u64(), first[1]["wbSeq"].as_u64()),
            (Some(1), Some(2))
        );
        // The reply was lost: the next poll still acknowledges nothing.
        assert_eq!(link.take(wait, Some(0)).await, first);
        link.push(json!({"type": "user", "n": 3})).unwrap();
        let next = link.take(wait, Some(2)).await;
        assert_eq!(next, vec![json!({"type": "user", "n": 3, "wbSeq": 3})]);
        assert_eq!(link.acked(), 2, "3 is delivered, not yet acknowledged");
        assert!(link.take(wait, Some(3)).await.is_empty());
    }

    #[tokio::test]
    async fn an_ack_past_what_was_delivered_drops_nothing_new() {
        let link = ModLink::new("t".into(), None);
        link.push(json!({"type": "user"})).unwrap();
        let got = link.take(Duration::from_millis(20), Some(99)).await;
        assert_eq!(got, vec![json!({"type": "user", "wbSeq": 1})]);
    }

    #[test]
    fn posted_lines_fold_once_however_often_they_come() {
        let link = ModLink::new("t".into(), None);
        let mut folded = Vec::new();
        let n = |i: i64| json!({"n": i});
        let a = Some("a");
        link.hello_from(a);
        link.fold_new(a, Some(1), &[n(1), n(2)], |l| folded.push(l["n"].clone()));
        // A retry after a lost reply carries them again, with the next one.
        link.fold_new(a, Some(1), &[n(1), n(2), n(3)], |l| {
            folded.push(l["n"].clone())
        });
        // A hung post that finally lands after a newer one.
        link.fold_new(a, Some(2), &[n(2)], |l| folded.push(l["n"].clone()));
        // Unnumbered (an older plugin, a withdraw past the queue) always folds.
        link.fold_new(None, None, &[n(9)], |l| folded.push(l["n"].clone()));
        assert_eq!(folded, vec![json!(1), json!(2), json!(3), json!(9)]);

        // The worker restarted: its numbers start again, and the old one's
        // late post is dropped.
        folded.clear();
        link.hello_from(Some("b"));
        link.hello_from(Some("b"));
        link.fold_new(Some("b"), Some(1), &[n(10)], |l| {
            folded.push(l["n"].clone())
        });
        link.fold_new(a, Some(4), &[n(4)], |l| folded.push(l["n"].clone()));
        assert_eq!(folded, vec![json!(10)]);
    }
}
