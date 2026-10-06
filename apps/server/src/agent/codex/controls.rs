//! Workbench actions, deliberately separate from model prompts.
use super::*;
use workbench_core::codex_controls::Action;

impl CodexDriver {
    pub(super) fn control_request(
        &mut self,
        request_id: &str,
        method: &str,
        params: Value,
        probe: bool,
    ) -> Value {
        self.request(
            method,
            params,
            Pending::Control {
                request_id: request_id.into(),
                method: method.into(),
                probe,
            },
        )
    }

    pub fn action(&mut self, request_id: &str, action: Action, p: &Value) -> Result<Effects> {
        let thread = self
            .thread_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Codex is still starting"))?;
        let string = |key: &str| -> Result<String> {
            let v = p
                .get(key)
                .and_then(Value::as_str)
                .filter(|v| !v.trim().is_empty() && v.len() <= 64 * 1024)
                .ok_or_else(|| anyhow::anyhow!("{key} is required"))?;
            Ok(v.into())
        };
        let mut fx = Effects::default();
        let (method, params) = match action {
            Action::Compact | Action::Review | Action::Fork
                if self.t.meta().busy || self.starting_turn =>
            {
                bail!("Finish or interrupt the current turn first")
            }
            Action::Compact => ("thread/compact/start", json!({"threadId":thread})),
            Action::Review => {
                let target = match p
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("uncommittedChanges")
                {
                    "uncommittedChanges" => json!({"type":"uncommittedChanges"}),
                    "baseBranch" => json!({"type":"baseBranch","branch":string("branch")?}),
                    "commit" => json!({"type":"commit","sha":string("sha")?}),
                    "custom" => json!({"type":"custom","instructions":string("instructions")?}),
                    _ => bail!("unknown review target"),
                };
                (
                    "review/start",
                    json!({"threadId":thread,"target":target,"delivery":"inline"}),
                )
            }
            Action::Fork => (
                "thread/fork",
                json!({"threadId":thread,"cwd":self.cwd,"excludeTurns":true,"deferGoalContinuation":true}),
            ),
            Action::Rename => (
                "thread/name/set",
                json!({"threadId":thread,"name":string("name")?}),
            ),
            Action::Archive => ("thread/archive", json!({"threadId":thread})),
            // Unarchive only a thread from the cwd-filtered list, checked in the callback.
            Action::Unarchive => {
                let id = string("threadId")?;
                if !self.listed_threads.contains(&id) {
                    bail!("Choose a thread from this folder first");
                }
                if !workbench_core::claude_transcript::is_uuid(&id) {
                    bail!("thread id must be a UUID");
                }
                ("thread/unarchive", json!({"threadId":id}))
            }
            Action::Threads => (
                "thread/list",
                json!({"cwd":self.cwd,"archived":p.get("archived").and_then(Value::as_bool).unwrap_or(false),"searchTerm":p.get("search").and_then(Value::as_str).unwrap_or(""),"cursor":p.get("cursor"),"sortKey":"updated_at","sortDirection":"desc","limit":50,"modelProviders":[]}),
            ),
            Action::History => {
                // Each attached device pages independently from the same initial boundary.
                let cursor = p
                    .get("cursor")
                    .filter(|v| !v.is_null())
                    .cloned()
                    .or_else(|| self.older_cursor.clone())
                    .ok_or_else(|| anyhow::anyhow!("No earlier history"))?;
                if cursor.as_str().filter(|s| s.len() <= 4096).is_none() {
                    bail!("Invalid history cursor");
                }
                fx.send.push(self.request(
                    "thread/items/list",
                    json!({"threadId":thread,"cursor":cursor,"sortDirection":"desc","limit":100}),
                    Pending::OlderHistory {
                        request_id: request_id.into(),
                    },
                ));
                return Ok(fx);
            }
            Action::Collaboration => {
                let mode = string("mode")?;
                if !self
                    .state
                    .collaboration_modes
                    .iter()
                    .any(|v| v["mode"] == mode)
                {
                    bail!("This Codex does not offer {mode} mode");
                }
                self.state.collaboration_mode = Some(mode);
                return Ok(self.local_result(request_id));
            }
            Action::ServiceTier => {
                let tier = string("tier")?;
                if tier != "default" {
                    let current = self.model.as_ref().or(self.t.meta().model.as_ref());
                    let option = self.t.meta().models.iter().find(|m| {
                        current
                            .is_some_and(|c| &m.value == c || m.resolved_model.as_ref() == Some(c))
                    });
                    if !option.is_some_and(|m| m.service_tiers.iter().any(|v| v["id"] == tier)) {
                        bail!("The selected model does not offer {tier}");
                    }
                }
                // Send an explicit reset: turn overrides are sticky.
                self.state.service_tier = Some(tier);
                return Ok(self.local_result(request_id));
            }
            Action::Goal => {
                let mut params = json!({"threadId":thread});
                if p.get("objective").is_some() {
                    params["objective"] = json!(string("objective")?);
                }
                if let Some(status) = p.get("status").and_then(Value::as_str) {
                    if !["active", "paused"].contains(&status) {
                        bail!("Goal status must be active or paused");
                    }
                    params["status"] = json!(status);
                }
                if let Some(budget) = p.get("tokenBudget") {
                    if !budget.is_null() && budget.as_u64().filter(|v| *v > 0).is_none() {
                        bail!("Token budget must be a positive integer");
                    }
                    params["tokenBudget"] = budget.clone();
                }
                ("thread/goal/set", params)
            }
            Action::ClearGoal => ("thread/goal/clear", json!({"threadId":thread})),
            Action::QueuePause => {
                self.state.queue_paused = p
                    .get("paused")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| anyhow::anyhow!("paused must be a boolean"))?;
                let mut fx = self.local_result(request_id);
                self.flush_followups(&mut fx);
                self.sync_state(&mut fx);
                return Ok(fx);
            }
            Action::QueueAdd => {
                if self.followups.len() >= 50 {
                    bail!("The queue is full (50 messages)");
                }
                let text = p
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if text.len() > 64 * 1024 {
                    bail!("Queued text must be under 64 KB");
                }
                let images: Vec<PromptImage> =
                    serde_json::from_value(p.get("images").cloned().unwrap_or(json!([])))?;
                if images.len() > super::super::MAX_IMAGES {
                    bail!("Too many images");
                }
                images.iter().try_for_each(PromptImage::validate)?;
                let files: Vec<PromptFile> =
                    serde_json::from_value(p.get("files").cloned().unwrap_or(json!([])))?;
                if files.len() > super::super::MAX_FILES {
                    bail!("Too many files");
                }
                files.iter().try_for_each(PromptFile::validate)?;
                if text.trim().is_empty() && images.is_empty() && files.is_empty() {
                    bail!("A queued message needs text or attachments");
                }
                let file_context = self.file_context(&files)?;
                let input = self.prompt_input(&format!("{text}{file_context}"), &images, &[])?;
                let existing: usize = self
                    .followups
                    .iter()
                    .map(|(_, input)| input.iter().map(|v| v.to_string().len()).sum::<usize>())
                    .sum();
                if existing + input.iter().map(|v| v.to_string().len()).sum::<usize>()
                    > 32 * 1024 * 1024
                {
                    bail!("The queue has reached its 32 MB limit");
                }
                self.followups.push((
                    QueuedPrompt {
                        id: uuid::Uuid::new_v4().to_string(),
                        text,
                        images: images.len(),
                        files: files.iter().map(|file| file.name.clone()).collect(),
                        file_context,
                    },
                    input,
                ));
                fx = self.local_result(request_id);
                self.flush_followups(&mut fx);
                self.sync_state(&mut fx);
                return Ok(fx);
            }
            Action::QueueUpdate
            | Action::QueueDelete
            | Action::QueueReorder
            | Action::QueueSend => {
                let id = string("id")?;
                let i = self
                    .followups
                    .iter()
                    .position(|(v, _)| v.id == id)
                    .ok_or_else(|| anyhow::anyhow!("Message was already sent or removed"))?;
                match action {
                    Action::QueueUpdate => {
                        let text = string("text")?;
                        self.followups[i].0.text = text.clone();
                        let mut input = self.prompt_input(
                            &format!("{text}{}", self.followups[i].0.file_context),
                            &[],
                            &[],
                        )?;
                        input.extend(
                            self.followups[i]
                                .1
                                .iter()
                                .filter(|v| v["type"] == "image")
                                .cloned(),
                        );
                        self.followups[i].1 = input;
                    }
                    Action::QueueDelete => {
                        self.followups.remove(i);
                    }
                    Action::QueueReorder => {
                        let direction = p
                            .get("direction")
                            .and_then(Value::as_i64)
                            .filter(|v| *v == -1 || *v == 1)
                            .ok_or_else(|| anyhow::anyhow!("direction must be -1 or 1"))?;
                        let target = (i as i64 + direction)
                            .clamp(0, self.followups.len() as i64 - 1)
                            as usize;
                        self.followups.swap(i, target);
                    }
                    Action::QueueSend => {
                        let (_, input) = self.followups.remove(i);
                        fx.send.extend(self.submit(input));
                    }
                    _ => unreachable!(),
                }
                fx.frames.extend(self.local_result(request_id).frames);
                self.sync_state(&mut fx);
                return Ok(fx);
            }
            Action::Inspect => {
                let section = string("section")?;
                match section.as_str() {
                    "account" => ("account/read", json!({"refreshToken":false})),
                    "usage" => ("account/rateLimits/read", json!({})),
                    "mcp" => (
                        "mcpServerStatus/list",
                        json!({"cursor":p.get("cursor"),"limit":50}),
                    ),
                    "plugins" => ("plugin/list", json!({"cwds":[self.cwd]})),
                    "hooks" => ("hooks/list", json!({"cwds":[self.cwd]})),
                    "remote" => ("remoteControl/status/read", json!({})),
                    "clients" => (
                        "remoteControl/client/list",
                        json!({"environmentId":p.get("environmentId").and_then(Value::as_str).or(self.remote_environment.as_deref()).ok_or_else(||anyhow::anyhow!("Enable native remote access first"))?,"limit":50,"cursor":p.get("cursor")}),
                    ),
                    "background" => (
                        "thread/backgroundTerminals/list",
                        json!({"threadId":thread}),
                    ),
                    "attachments" => ("thread/attachment/list", json!({"threadId":thread})),
                    "voices" => ("thread/realtime/listVoices", json!({})),
                    "goal" => ("thread/goal/get", json!({"threadId":thread})),
                    "task" => {
                        let id = string("threadId")?;
                        if !self.t.meta().tasks.iter().any(|t| t.id == id) {
                            bail!("Unknown child thread");
                        }
                        (
                            "thread/items/list",
                            json!({"threadId":id,"sortDirection":"desc","limit":100}),
                        )
                    }
                    _ => bail!("unknown Codex status section"),
                }
            }
            Action::Login => ("account/login/start", json!({"type":"chatgpt"})),
            Action::CancelLogin => (
                "account/login/cancel",
                json!({"loginId":string("loginId")?}),
            ),
            Action::McpLogin => (
                "mcpServer/oauth/login",
                json!({"name":string("name")?,"threadId":thread}),
            ),
            // Explicit UI actions only. Ephemeral pairing belongs to this owned
            // process; Workbench never stops a user's shared daemon.
            Action::RemoteEnable => ("remoteControl/enable", json!({"ephemeral":true})),
            Action::RemoteDisable => ("remoteControl/disable", json!({})),
            Action::RemotePair => ("remoteControl/pairing/start", json!({"manualCode":true})),
            Action::RemoteRevoke => (
                "remoteControl/client/revoke",
                json!({"environmentId":string("environmentId")?,"clientId":string("clientId")?}),
            ),
            Action::BackgroundTerminate => (
                "thread/backgroundTerminals/terminate",
                json!({"threadId":thread,"processId":string("processId")?}),
            ),
            Action::BackgroundClean => (
                "thread/backgroundTerminals/clean",
                json!({"threadId":thread}),
            ),
            Action::RealtimeStart => (
                "thread/realtime/start",
                json!({"threadId":thread,"outputModality":"audio","transport":{"type":"websocket"}}),
            ),
            Action::RealtimeStop => ("thread/realtime/stop", json!({"threadId":thread})),
            Action::RealtimeText => (
                "thread/realtime/appendText",
                json!({"threadId":thread,"text":string("text")?}),
            ),
            Action::RealtimeAudio => {
                let data = string("data")?;
                let sample_rate = p
                    .get("sampleRate")
                    .and_then(Value::as_u64)
                    .filter(|v| (8000..=96000).contains(v))
                    .ok_or_else(|| anyhow::anyhow!("Invalid audio sample rate"))?;
                (
                    "thread/realtime/appendAudio",
                    json!({"threadId":thread,"audio":{"data":data,"sampleRate":sample_rate,"numChannels":1,"samplesPerChannel":null,"itemId":null}}),
                )
            }
            Action::Elicitation => {
                let id = string("id")?;
                let choice = string("choice")?;
                let (i, response) =
                    self.t
                        .resolve_elicitation_checked(&id, &choice, p.get("content"))?;
                fx.send.push(response);
                fx.items.push(i);
                fx.frames
                    .push(json!({"t":"codexResult","requestId":request_id,"result":{}}));
                return Ok(fx);
            }
        };
        fx.send
            .push(self.control_request(request_id, method, params, false));
        Ok(fx)
    }

    fn local_result(&mut self, request_id: &str) -> Effects {
        let mut fx = Effects {
            frames: vec![json!({"t":"codexResult","requestId":request_id,"result":{}})],
            ..Effects::default()
        };
        self.sync_state(&mut fx);
        fx
    }
    pub(super) fn sync_state(&mut self, fx: &mut Effects) {
        self.state.queue = self.followups.iter().map(|(q, _)| q.clone()).collect();
        self.state.has_older_history = self.older_cursor.is_some();
        if self.t.meta().codex.as_ref() != Some(&self.state) {
            self.t.set_codex_state(self.state.clone());
            fx.meta = true;
        }
    }
    pub(super) fn flush_followups(&mut self, fx: &mut Effects) {
        if !self.state.queue_paused
            && !self.starting_turn
            && !self.t.meta().busy
            && !self.followups.is_empty()
        {
            let followup = self.followups.remove(0);
            fx.send.extend(self.submit(followup.1.clone()));
            self.inflight_followup = Some(followup);
            self.t.set_busy();
            fx.meta = true;
        }
    }
    pub(super) fn control_result(
        &mut self,
        request_id: &str,
        method: &str,
        probe: bool,
        result: &Value,
        fx: &mut Effects,
    ) {
        if !self.state.capabilities.iter().any(|m| m == method) {
            self.state.capabilities.push(method.into());
        }
        match method {
            "remoteControl/status/read"
            | "remoteControl/enable"
            | "remoteControl/pairing/start" => {
                self.remote_environment = result
                    .get("environmentId")
                    .and_then(Value::as_str)
                    .map(String::from)
            }
            "thread/list" => {
                for thread in result
                    .get("data")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(id) = thread.get("id").and_then(Value::as_str) {
                        self.listed_threads.insert(id.into());
                    }
                }
            }
            "collaborationMode/list" => {
                self.state.collaboration_modes =
                    result["data"].as_array().cloned().unwrap_or_default()
            }
            "thread/goal/get" | "thread/goal/set" => {
                self.state.goal = result.get("goal").filter(|v| !v.is_null()).cloned()
            }
            "thread/goal/clear" => self.state.goal = None,
            "account/rateLimits/read" => {
                let applied = self
                    .t
                    .apply(&json!({"method":"account/rateLimits/updated","params":result}));
                fx.meta |= applied.meta;
            }
            "thread/realtime/start" => self.state.realtime = true,
            "thread/realtime/stop" => self.state.realtime = false,
            _ => {}
        }
        if method == "thread/fork" {
            if let Some(id) = result.pointer("/thread/id").and_then(Value::as_str) {
                fx.send.push(self.request(
                    "thread/unsubscribe",
                    json!({"threadId":id}),
                    Pending::ForkDetached {
                        request_id: request_id.into(),
                        result: result.clone(),
                    },
                ));
                return;
            }
        }
        if !probe {
            fx.frames
                .push(json!({"t":"codexResult","requestId":request_id,"result":result}));
        }
    }
    pub(super) fn sync_notifications(&mut self, msg: &Value, fx: &mut Effects) {
        let p = &msg["params"];
        match msg["method"].as_str() {
            Some("thread/goal/updated") => self.state.goal = p.get("goal").cloned(),
            Some("thread/goal/cleared") => self.state.goal = None,
            Some("thread/settings/updated") => {
                if p.get("approvalPolicy").is_some() {
                    self.state.approval_policy = p.get("approvalPolicy").cloned();
                }
                if p.get("sandbox").is_some() {
                    self.state.sandbox = p.get("sandbox").cloned();
                }
            }
            Some(method) if method.starts_with("thread/realtime/") => {
                if method.ends_with("closed") || method.ends_with("error") {
                    self.state.realtime = false;
                }
                fx.frames
                    .push(json!({"t":"codexEvent","method":method,"params":p}));
            }
            Some(
                "account/login/completed"
                | "remoteControl/status/changed"
                | "mcpServer/oauthLogin/completed",
            ) => fx
                .frames
                .push(json!({"t":"codexEvent","method":msg["method"],"params":p})),
            _ => {}
        }
    }
    pub fn tick(&mut self) -> Effects {
        let now = Instant::now();
        let expired: Vec<_> = self
            .deadlines
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(id, _)| *id)
            .collect();
        let mut fx = Effects::default();
        for id in expired {
            self.deadlines.remove(&id);
            if let Some(pending) = self.pending.remove(&id) {
                self.on_error(
                    pending,
                    &json!({"message":"Codex request timed out after 30 seconds"}),
                    &mut fx,
                );
            }
        }
        self.sync_state(&mut fx);
        fx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn driver() -> CodexDriver {
        super::super::tests::driver()
    }
    #[test]
    fn queued_followups_wait_can_be_edited_reordered_and_start_once() {
        let mut d = driver();
        let start = d.prompt("first", &[], &[]).unwrap();
        d.apply_line(
            &json!({"id":start.send[0]["id"],"result":{"turn":{"id":"turn"}}}).to_string(),
        );
        assert!(d
            .action("a", Action::QueueAdd, &json!({"text":"second"}))
            .unwrap()
            .send
            .is_empty());
        d.action("b", Action::QueueAdd, &json!({"text":"third"}))
            .unwrap();
        let second = d.state.queue[0].id.clone();
        let third = d.state.queue[1].id.clone();
        d.action(
            "c",
            Action::QueueUpdate,
            &json!({"id":second,"text":"edited"}),
        )
        .unwrap();
        d.action(
            "d",
            Action::QueueReorder,
            &json!({"id":third,"direction":-1}),
        )
        .unwrap();
        let fx=d.apply_line(&json!({"method":"turn/completed","params":{"turn":{"id":"turn","status":"completed"}}}).to_string());
        assert_eq!(fx.send[0]["method"], "turn/start");
        assert_eq!(fx.send[0]["params"]["input"][0]["text"], "third");
        assert_eq!(d.state.queue.len(), 1);
        assert!(d.tick().send.is_empty());
        d.action("e", Action::QueueDelete, &json!({"id":second}))
            .unwrap();
        assert!(d.state.queue.is_empty());
    }
    #[test]
    fn interrupt_preserves_followups_until_queue_is_resumed() {
        let mut d = driver();
        let start = d.prompt("first", &[], &[]).unwrap();
        d.apply_line(
            &json!({"id":start.send[0]["id"],"result":{"turn":{"id":"turn"}}}).to_string(),
        );
        d.action("a", Action::QueueAdd, &json!({"text":"second"}))
            .unwrap();
        d.interrupt();
        let completed=d.apply_line(&json!({"method":"turn/completed","params":{"turn":{"id":"turn","status":"interrupted"}}}).to_string());
        assert!(completed.send.is_empty());
        assert_eq!(d.state.queue.len(), 1);
        assert!(d.state.queue_paused);
        let resume = d
            .action("r", Action::QueuePause, &json!({"paused":false}))
            .unwrap();
        assert_eq!(resume.send[0]["params"]["input"][0]["text"], "second");
    }
    #[test]
    fn attached_clients_page_history_independently() {
        let mut d = driver();
        d.older_cursor = Some(json!("initial"));
        let first = d.action("one", Action::History, &json!({})).unwrap();
        let second = d.action("two", Action::History, &json!({})).unwrap();
        assert_eq!(first.send[0]["params"]["cursor"], "initial");
        assert_eq!(second.send[0]["params"]["cursor"], "initial");
        let reply = d.apply_line(
            &json!({"id":first.send[0]["id"],"result":{"data":[],"nextCursor":"older"}})
                .to_string(),
        );
        assert_eq!(reply.frames[0]["result"]["nextCursor"], "older");
        assert_eq!(d.older_cursor, Some(json!("initial")));
        let next = d
            .action("three", Action::History, &json!({"cursor":"older"}))
            .unwrap();
        assert_eq!(next.send[0]["params"]["cursor"], "older");
    }
    #[test]
    fn failed_queued_turn_is_retained_and_paused_instead_of_lost_or_retried_forever() {
        let mut d = driver();
        let queued = d
            .action("q", Action::QueueAdd, &json!({"text":"keep me"}))
            .unwrap();
        assert_eq!(queued.send[0]["method"], "turn/start");
        d.apply_line(&json!({"id":queued.send[0]["id"],"error":{"message":"Busy"}}).to_string());
        assert_eq!(d.state.queue[0].text, "keep me");
        assert!(d.state.queue_paused);
        assert!(d.tick().send.is_empty());
    }
    #[test]
    fn queued_files_survive_edits_and_use_the_shared_attachment_store() {
        let mut d = driver();
        d.thread_id = Some(uuid::Uuid::new_v4().to_string());
        d.state.queue_paused = true;
        d.action("q", Action::QueueAdd, &json!({"text":"read it","files":[{"name":"README.md","mediaType":"text/plain","data":"Project context"}]})).unwrap();
        let queued = &d.followups[0];
        assert_eq!(queued.0.files, ["README.md"]);
        assert!(queued.1[0]["text"]
            .as_str()
            .unwrap()
            .contains("Read them with your tools"));
        let context = queued.0.file_context.clone();
        let id = queued.0.id.clone();
        d.action(
            "edit",
            Action::QueueUpdate,
            &json!({"id":id,"text":"new instructions"}),
        )
        .unwrap();
        assert_eq!(
            d.followups[0].1[0]["text"],
            format!("new instructions{context}")
        );
        let dir = super::super::super::attachment::attachment_dir(d.thread_id.as_ref().unwrap());
        let upload = std::fs::read_dir(&dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            std::fs::read_to_string(upload.join("1-README.md")).unwrap(),
            "Project context"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn queued_skills_and_image_only_messages_keep_native_input_semantics() {
        let mut d = driver();
        d.state.queue_paused = true;
        d.skills
            .insert("review".into(), "/skills/review/SKILL.md".into());
        d.action("q", Action::QueueAdd, &json!({"text":"/review changes"}))
            .unwrap();
        assert_eq!(d.followups[0].1[0]["type"], "skill");
        let id = d.followups[0].0.id.clone();
        d.action(
            "e",
            Action::QueueUpdate,
            &json!({"id":id,"text":"ordinary text"}),
        )
        .unwrap();
        assert_eq!(d.followups[0].1[0]["type"], "text");
        d.action(
            "i",
            Action::QueueAdd,
            &json!({"images":[{"mediaType":"image/png","data":"iVBORw=="}]}),
        )
        .unwrap();
        assert_eq!(d.followups[1].1[0]["type"], "image");
    }
    #[test]
    fn fork_detaches_before_opening_another_pane_and_foreign_turns_do_not_mutate_original() {
        let mut d = driver();
        let fork = d.action("fork", Action::Fork, &json!({})).unwrap();
        let response = d.apply_line(
            &json!({"id":fork.send[0]["id"],"result":{"thread":{"id":"forked"}}}).to_string(),
        );
        assert!(response.frames.is_empty());
        assert_eq!(response.send[0]["method"], "thread/unsubscribe");
        d.apply_line(&json!({"method":"turn/started","params":{"threadId":"forked","turn":{"id":"foreign"}}}).to_string());
        assert!(!d.t.meta().busy);
        let detached = d.apply_line(
            &json!({"id":response.send[0]["id"],"result":{"status":"unsubscribed"}}).to_string(),
        );
        assert_eq!(detached.frames[0]["result"]["thread"]["id"], "forked");
        assert_eq!(d.thread_id.as_deref(), Some("thread-1"));
    }
    #[test]
    fn independent_policy_and_sandbox_overrides_reach_the_turn() {
        let mut d = driver();
        d.options = LaunchOptions {
            codex_approval_policy: Some("never".into()),
            codex_sandbox_mode: Some("read-only".into()),
        };
        let fx = d.prompt("hello", &[], &[]).unwrap();
        assert_eq!(fx.send[0]["params"]["approvalPolicy"], "never");
        assert_eq!(fx.send[0]["params"]["sandboxPolicy"]["type"], "readOnly");
        d.set_mode("auto").unwrap();
        assert!(d.options.codex_approval_policy.is_none());
    }
    #[test]
    fn unsupported_optional_probes_do_not_fail_start_and_actions_report_errors() {
        let mut d = driver();
        let probe = d.control_request("", "thread/goal/get", json!({}), true);
        let fx = d.apply_line(
            &json!({"id":probe["id"],"error":{"code":-32601,"message":"Unsupported"}}).to_string(),
        );
        assert!(fx.ready.is_none());
        assert!(fx.frames.is_empty());
        let fx = d.action("compact", Action::Compact, &json!({})).unwrap();
        let reply = d.apply_line(
            &json!({"id":fx.send[0]["id"],"error":{"code":-32601,"message":"Unsupported"}})
                .to_string(),
        );
        assert_eq!(reply.frames[0]["requestId"], "compact");
        assert_eq!(reply.frames[0]["error"], "Unsupported");
    }
    #[test]
    fn catalog_paginates_and_incompatible_sticky_effort_is_explicitly_reset() {
        let mut d = driver();
        let req = d.request("model/list", json!({}), Pending::Models);
        let fx=d.apply_line(&json!({"id":req["id"],"result":{"data":[{"id":"one","model":"one","defaultReasoningEffort":"high","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}],"nextCursor":"next"}}).to_string());
        assert_eq!(fx.send[0]["params"]["cursor"], "next");
        d.apply_line(&json!({"id":fx.send[0]["id"],"result":{"data":[{"id":"two","model":"two","defaultReasoningEffort":"low","supportedReasoningEfforts":[{"reasoningEffort":"low"}]}],"nextCursor":null}}).to_string());
        d.set_model("one").unwrap();
        d.set_effort("high").unwrap();
        d.set_model("two").unwrap();
        let fx = d.prompt("next", &[], &[]).unwrap();
        assert_eq!(fx.send[0]["params"]["effort"], "low");
        assert_eq!(d.t.meta().models.len(), 2);
    }
    #[test]
    fn expired_turn_rpc_recovers_and_goal_budget_is_validated() {
        let mut d = driver();
        let fx = d.prompt("hello", &[], &[]).unwrap();
        let id = fx.send[0]["id"].as_u64().unwrap();
        d.deadlines
            .insert(id, Instant::now() - Duration::from_secs(1));
        let fx = d.tick();
        assert!(!d.starting_turn);
        assert!(!d.t.meta().busy);
        assert!(!fx.items.is_empty());
        assert!(d
            .action(
                "goal",
                Action::Goal,
                &json!({"objective":"finish","tokenBudget":-1})
            )
            .is_err());
        let fx = d
            .action(
                "goal",
                Action::Goal,
                &json!({"objective":"finish","tokenBudget":100}),
            )
            .unwrap();
        assert_eq!(fx.send[0]["method"], "thread/goal/set");
        assert!(d
            .action("bad", Action::Inspect, &json!({"section":"arbitrary/rpc"}))
            .is_err());
    }
}
