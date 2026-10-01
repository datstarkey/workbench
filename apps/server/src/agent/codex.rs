//! The Codex driver: `codex app-server`, JSON-RPC 2.0 over stdio (one object
//! per line). This side owns the request bookkeeping — ids, the handshake,
//! starting or resuming the thread, history, turns — and leaves folding what
//! codex says to [`CodexTranscript`]. The session id is the thread id, so a
//! chat can be resumed later (or in `codex resume <id>`), one process at a time.
//!
//! Start-up: `initialize`, then `initialized` + `thread/start` (or
//! `thread/resume` and a backwards `thread/items/list` for history) +
//! `model/list`. The session is ready once the thread id (and history) is in.

use std::collections::HashMap;

use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use workbench_core::claude_transcript::{ApprovalDecision, ChatView};
use workbench_core::codex_config;
use workbench_core::codex_transcript::{self, CodexTranscript, CODEX_MODES};

use super::driver::{Driver, Effects, Launch};
use super::session::SNAPSHOT_ITEMS;
use super::{PromptImage, StartAgent};

pub(super) fn validate(thread_id: Option<&str>, mode: Option<&str>) -> Result<()> {
    if thread_id.is_some_and(|id| !workbench_core::claude_transcript::is_uuid(id)) {
        bail!("session id must be a UUID");
    }
    if let Some(mode) = mode.filter(|m| !CODEX_MODES.contains(m)) {
        bail!("unknown Codex mode: {mode}");
    }
    Ok(())
}

/// What a response answers.
#[derive(Debug)]
enum Pending {
    Initialize,
    Thread,
    Models,
    History,
    TurnStart,
    /// The input, re-submitted if the turn it steered has ended.
    Steer {
        input: Vec<Value>,
        turn: String,
    },
    Interrupt,
}

pub(super) struct CodexDriver {
    t: CodexTranscript,
    cwd: String,
    resume: Option<String>,
    thread_id: Option<String>,
    next_id: u64,
    pending: HashMap<u64, Pending>,
    history: Vec<Value>,
    /// A `turn/start` is in flight: prompts wait for its turn id to steer it.
    starting_turn: bool,
    /// Stop was pressed while `turn/start` was in flight: interrupt the turn
    /// as soon as it has an id.
    interrupt_on_start: bool,
    queued: Vec<Value>,
    /// What the person picked; sent with every `turn/start` (codex keeps them
    /// for later turns too, so repeating is harmless).
    mode: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

/// `thread_id` resumes that thread; `None` starts a new one.
pub(super) fn launch(req: &StartAgent, thread_id: Option<&str>, mode: Option<&str>) -> Launch {
    let mut cmd = super::session::base_command(codex_config::codex_binary(), req);
    cmd.arg("app-server");
    let mut driver = CodexDriver {
        t: CodexTranscript::default(),
        cwd: req.cwd.clone(),
        resume: thread_id.map(String::from),
        thread_id: None,
        next_id: 0,
        pending: HashMap::new(),
        history: Vec::new(),
        starting_turn: false,
        interrupt_on_start: false,
        queued: Vec::new(),
        mode: mode.map(String::from),
        model: None,
        effort: None,
    };
    let hello = driver.request(
        "initialize",
        json!({
            "clientInfo": {
                "name": "workbench",
                "title": "Workbench",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {"experimentalApi": false, "requestAttestation": false},
        }),
        Pending::Initialize,
    );
    Launch {
        cmd,
        driver: Driver::Codex(driver),
        hello: vec![hello],
        ready: None,
        program: "codex",
    }
}

impl CodexDriver {
    pub fn transcript(&self) -> &CodexTranscript {
        &self.t
    }

    fn request(&mut self, method: &str, params: Value, pending: Pending) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, pending);
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    }

    pub fn apply_line(&mut self, line: &str) -> Effects {
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            return Effects::default();
        };
        let mut fx = Effects::default();
        if msg.get("method").is_some() {
            let applied = self.t.apply(&msg);
            if let Some(method) = &applied.unknown_method {
                tracing::warn!("codex sent an unrecognised notification: {method}");
            }
            fx.send.extend(applied.reply);
            fx.items = applied.items;
            fx.meta = applied.meta;
        } else if let Some(pending) = msg
            .get("id")
            .and_then(Value::as_u64)
            .and_then(|id| self.pending.remove(&id))
        {
            match msg.get("error") {
                Some(error) => self.on_error(pending, error, &mut fx),
                None => self.on_result(pending, msg.get("result").unwrap_or(&Value::Null), &mut fx),
            }
        }
        self.flush_queue(&mut fx);
        fx
    }

    fn on_result(&mut self, pending: Pending, result: &Value, fx: &mut Effects) {
        match pending {
            Pending::Initialize => {
                fx.send
                    .push(json!({"jsonrpc": "2.0", "method": "initialized"}));
                let mut params = json!({"cwd": self.cwd});
                if let Some((policy, sandbox)) = self
                    .mode
                    .as_deref()
                    .and_then(codex_transcript::sandbox_mode)
                {
                    params["approvalPolicy"] = json!(policy);
                    params["sandbox"] = json!(sandbox);
                }
                let thread = match &self.resume {
                    Some(id) => {
                        params["threadId"] = json!(id);
                        params["excludeTurns"] = json!(true);
                        self.request("thread/resume", params, Pending::Thread)
                    }
                    None => self.request("thread/start", params, Pending::Thread),
                };
                fx.send.push(thread);
                fx.send
                    .push(self.request("model/list", json!({}), Pending::Models));
            }
            Pending::Thread => {
                let Some(id) = result.pointer("/thread/id").and_then(Value::as_str) else {
                    fx.ready = Some(Err("codex didn't return a thread id".into()));
                    return;
                };
                self.thread_id = Some(id.to_string());
                self.t.apply_thread(result);
                fx.meta = true;
                match result.get("itemsBackwardsCursor").filter(|c| !c.is_null()) {
                    Some(cursor) if self.resume.is_some() => {
                        let cursor = cursor.clone();
                        fx.send.push(self.history_page(cursor));
                    }
                    _ => fx.ready = Some(Ok(id.to_string())),
                }
            }
            Pending::History => {
                let page = result.get("data").and_then(Value::as_array);
                self.history.extend(page.into_iter().flatten().cloned());
                match result.get("nextCursor").filter(|c| !c.is_null()) {
                    Some(next) if self.history.len() < SNAPSHOT_ITEMS => {
                        let next = next.clone();
                        fx.send.push(self.history_page(next));
                    }
                    _ => self.history_loaded(fx),
                }
            }
            Pending::Models => {
                let data = result.get("data").and_then(Value::as_array);
                self.t
                    .set_models(data.map(Vec::as_slice).unwrap_or_default());
                fx.meta = true;
            }
            Pending::TurnStart => {
                self.starting_turn = false;
                if let Some(turn) = result.pointer("/turn/id").and_then(Value::as_str) {
                    self.t.turn_started(turn);
                    fx.meta = true;
                }
                if std::mem::take(&mut self.interrupt_on_start) {
                    let stop = self.interrupt();
                    fx.send.extend(stop.send);
                    fx.items.extend(stop.items);
                }
            }
            Pending::Steer { .. } | Pending::Interrupt => {}
        }
    }

    fn on_error(&mut self, pending: Pending, error: &Value, fx: &mut Effects) {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("codex refused the request")
            .to_string();
        match pending {
            Pending::Initialize | Pending::Thread => fx.ready = Some(Err(message)),
            Pending::History => {
                tracing::warn!("codex history: {message}");
                fx.items = self
                    .t
                    .notice("history-error", "Couldn't load earlier messages.");
                self.history_loaded(fx);
            }
            Pending::TurnStart => {
                self.starting_turn = false;
                self.interrupt_on_start = false;
                let message = if std::mem::take(&mut self.queued).is_empty() {
                    message
                } else {
                    format!("{message} Messages sent after it weren't delivered either.")
                };
                self.t.set_idle();
                fx.items = self
                    .t
                    .notice(&format!("rejected:{}", self.next_id), &message);
                fx.meta = true;
            }
            // The turn ended before the steer landed: start a new one with it.
            Pending::Steer { input, turn } if self.t.active_turn() != Some(turn.as_str()) => {
                fx.send.extend(self.submit(input));
            }
            Pending::Steer { .. } => {
                fx.items = self
                    .t
                    .notice(&format!("rejected:{}", self.next_id), &message);
            }
            Pending::Models | Pending::Interrupt => {
                tracing::warn!("codex: {message}");
            }
        }
    }

    fn history_page(&mut self, cursor: Value) -> Value {
        let thread = self.thread_id.clone().unwrap_or_default();
        let limit = SNAPSHOT_ITEMS.saturating_sub(self.history.len()).max(1);
        let params =
            json!({"threadId": thread, "cursor": cursor, "sortDirection": "desc", "limit": limit});
        self.request("thread/items/list", params, Pending::History)
    }

    /// History came newest first; the transcript wants it oldest first.
    fn history_loaded(&mut self, fx: &mut Effects) {
        let mut entries = std::mem::take(&mut self.history);
        entries.reverse();
        self.t.load_history(&entries);
        fx.snapshot = true;
        fx.ready = self.thread_id.clone().map(Ok);
    }

    /// Start a turn with `input`, steer the running one, or (while a start
    /// is in flight) hold it until that turn has an id.
    fn submit(&mut self, input: Vec<Value>) -> Option<Value> {
        let thread = self.thread_id.clone()?;
        if let Some(turn) = self.t.active_turn().map(String::from) {
            let params = json!({"threadId": thread, "input": input, "expectedTurnId": turn});
            return Some(self.request("turn/steer", params, Pending::Steer { input, turn }));
        }
        if self.starting_turn {
            self.queued.extend(input);
            return None;
        }
        let mut params = json!({"threadId": thread, "input": input});
        if let Some(mode) = &self.mode {
            let (policy, _) = codex_transcript::sandbox_mode(mode)?;
            params["approvalPolicy"] = json!(policy);
            params["sandboxPolicy"] = codex_transcript::sandbox_policy(mode)?;
        }
        if let Some(model) = &self.model {
            params["model"] = json!(model);
        }
        if let Some(effort) = &self.effort {
            params["effort"] = json!(effort);
        }
        self.starting_turn = true;
        Some(self.request("turn/start", params, Pending::TurnStart))
    }

    fn flush_queue(&mut self, fx: &mut Effects) {
        if self.queued.is_empty() || self.starting_turn {
            return;
        }
        let input = std::mem::take(&mut self.queued);
        fx.send.extend(self.submit(input));
    }

    pub fn prompt(&mut self, text: &str, images: &[PromptImage]) -> Result<Effects> {
        if self.thread_id.is_none() {
            bail!("Codex is still starting");
        }
        let mut input = Vec::new();
        if !text.trim().is_empty() {
            input.push(json!({"type": "text", "text": text, "text_elements": []}));
        }
        // Verified with codex-cli 0.159: data URLs are accepted as-is.
        input.extend(images.iter().map(|img| {
            json!({"type": "image", "url": format!("data:{};base64,{}", img.media_type, img.data)})
        }));
        self.t.set_busy();
        Ok(Effects {
            send: self.submit(input).into_iter().collect(),
            meta: true,
            ..Effects::default()
        })
    }

    pub fn approve(
        &mut self,
        request_id: &str,
        decision: ApprovalDecision,
        answers: Option<&Map<String, Value>>,
    ) -> Effects {
        match self.t.resolve_approval(request_id, decision, answers) {
            Some((i, response)) => Effects {
                send: vec![response],
                items: vec![i],
                ..Effects::default()
            },
            None => Effects::default(),
        }
    }

    /// Withdraw open approvals, then stop the turn — once it has an id, if
    /// its start is still in flight. A no-op while idle.
    pub fn interrupt(&mut self) -> Effects {
        let (items, mut send) = self.t.cancel_approvals();
        self.queued.clear();
        match (self.thread_id.clone(), self.t.active_turn()) {
            (Some(thread), Some(turn)) => {
                let params = json!({"threadId": thread, "turnId": turn});
                send.push(self.request("turn/interrupt", params, Pending::Interrupt));
            }
            _ if self.starting_turn => self.interrupt_on_start = true,
            _ => {}
        }
        Effects {
            send,
            items,
            ..Effects::default()
        }
    }

    /// Applied from the next turn on.
    pub fn set_mode(&mut self, mode: &str) -> Result<Effects> {
        if !CODEX_MODES.contains(&mode) {
            bail!("unknown Codex mode: {mode}");
        }
        self.mode = Some(mode.to_string());
        self.t.set_permission_mode(mode);
        Ok(meta_changed())
    }

    pub fn set_model(&mut self, model: &str) -> Result<Effects> {
        let Some(option) = self.t.meta().models.iter().find(|m| m.value == model) else {
            bail!("unknown model: {model}");
        };
        // An effort the new model lacks would fail the next turn.
        let keeps_effort = self
            .effort
            .as_ref()
            .is_none_or(|e| option.effort_levels.contains(e));
        if !keeps_effort {
            self.effort = None;
            self.t.set_effort(None);
        }
        self.model = Some(model.to_string());
        self.t.set_model_choice(model);
        Ok(meta_changed())
    }

    pub fn set_effort(&mut self, level: &str) -> Result<Effects> {
        let meta = self.t.meta();
        let current = meta.model_choice.as_ref().or(meta.model.as_ref());
        let levels = current
            .and_then(|c| {
                meta.models
                    .iter()
                    .find(|m| &m.value == c || m.resolved_model.as_ref() == Some(c))
            })
            .map(|m| m.effort_levels.clone())
            .unwrap_or_default();
        if !levels.iter().any(|l| l == level) {
            bail!("unknown effort level: {level}");
        }
        self.effort = Some(level.to_string());
        self.t.set_effort(Some(level));
        Ok(meta_changed())
    }
}

fn meta_changed() -> Effects {
    Effects {
        meta: true,
        ..Effects::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn driver() -> CodexDriver {
        CodexDriver {
            t: CodexTranscript::default(),
            cwd: "/tmp".into(),
            resume: None,
            thread_id: Some("thread-1".into()),
            next_id: 0,
            pending: HashMap::new(),
            history: Vec::new(),
            starting_turn: false,
            interrupt_on_start: false,
            queued: Vec::new(),
            mode: None,
            model: None,
            effort: None,
        }
    }

    #[test]
    fn stop_while_the_turn_is_starting_interrupts_it_once_it_has_an_id() {
        let mut d = driver();
        let start = d.prompt("hi", &[]).unwrap();
        assert_eq!(start.send[0]["method"], "turn/start");
        let start_id = start.send[0]["id"].as_u64().unwrap();

        assert!(d.interrupt().send.is_empty(), "no turn id to interrupt yet");

        let fx = d.apply_line(
            &json!({"jsonrpc": "2.0", "id": start_id, "result": {"turn": {"id": "turn-1"}}})
                .to_string(),
        );
        let stop = fx
            .send
            .iter()
            .find(|m| m["method"] == "turn/interrupt")
            .expect("the started turn is interrupted");
        assert_eq!(stop["params"]["turnId"], "turn-1");
    }
}
