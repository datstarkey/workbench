use super::items::{iso_utc, strip_shell};
use super::*;
use crate::claude_transcript::ApprovalDecision;
use serde_json::json;

const FIXTURE: &str = include_str!("fixtures/app-server-0.159.3.jsonl");

fn note(method: &str, params: Value) -> Value {
    json!({"method": method, "params": params})
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"method": method, "id": id, "params": params})
}

/// Feed every notification and request of the recorded turn, answering
/// approvals as a host would; returns the replies.
fn replay(t: &mut CodexTranscript, decision: ApprovalDecision) -> Vec<Value> {
    let mut sent = Vec::new();
    for line in FIXTURE.lines() {
        let msg: Value = serde_json::from_str(line).unwrap();
        if msg.get("method").is_none() {
            continue; // responses to the host's own requests
        }
        let applied = t.apply(&msg);
        assert!(applied.reply.is_none(), "approvals wait for the person");
        if msg.get("id").is_some() {
            let id = t.waiting_on().unwrap().id().to_string();
            sent.push(t.resolve_approval(&id, decision, None).unwrap().1);
        }
    }
    sent
}

#[test]
fn folds_a_recorded_turn() {
    let mut t = CodexTranscript::default();
    let sent = replay(&mut t, ApprovalDecision::Allow);
    assert_eq!(
        sent,
        vec![
            json!({"jsonrpc":"2.0","id":0,"result":{"decision":"accept"}}),
            json!({"jsonrpc":"2.0","id":1,"result":{"decision":"accept"}}),
        ]
    );
    assert!(t
        .items()
        .iter()
        .any(|i| matches!(i, TranscriptItem::Notice { .. })));
    let items: Vec<_> = t
        .items()
        .iter()
        .filter(|i| !matches!(i, TranscriptItem::Notice { .. }))
        .cloned()
        .collect();
    let kinds: Vec<&str> = items
        .iter()
        .map(|i| match i {
            TranscriptItem::User { .. } => "user",
            TranscriptItem::Text { .. } => "text",
            TranscriptItem::Tool { name, .. } => name,
            TranscriptItem::Approval { .. } => "approval",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        ["user", "text", "Bash", "approval", "Write", "approval", "text"]
    );
    let TranscriptItem::User {
        text,
        images,
        timestamp,
        ..
    } = &items[0]
    else {
        panic!()
    };
    assert!(text.starts_with("Ignore the image."));
    assert_eq!(*images, 1);
    assert_eq!(timestamp, "2026-10-01T18:41:04.596Z");
    assert_eq!(
        items[1],
        TranscriptItem::Text {
            id: "msg_0ea25b3050922329016abea942fe0c87d28fbb148f90825439".into(),
            text: "I’ll request write access for the command, then create b.txt.\n".into(),
        }
    );
    let TranscriptItem::Tool {
        input,
        status,
        output,
        ..
    } = &items[2]
    else {
        panic!()
    };
    assert_eq!(input["command"], "echo hi > note.txt");
    assert_eq!(input["cwd"], "/work/repo");
    assert_eq!(*status, ToolStatus::Ok);
    assert_eq!(*output, None);
    let TranscriptItem::Approval {
        tool,
        input,
        description,
        can_always_allow,
        decision,
        ..
    } = &items[3]
    else {
        panic!()
    };
    assert_eq!(tool, "Bash");
    assert_eq!(input["command"], "echo hi > note.txt");
    assert_eq!(
        description.as_deref(),
        Some("May I run `echo hi > note.txt` with write access?")
    );
    assert!(
        !can_always_allow,
        "only an execpolicy amendment (a lasting rule) is on offer, not acceptForSession"
    );
    assert_eq!(*decision, Some(ApprovalDecision::Allow));
    let TranscriptItem::Tool {
        input,
        patch,
        status,
        ..
    } = &items[4]
    else {
        panic!()
    };
    assert_eq!(input["file_path"], "/work/repo/b.txt");
    assert_eq!(input["content"], "x\n");
    assert_eq!(
        *patch,
        Some(json!([{"oldStart":0,"newStart":1,"lines":["+x"]}]))
    );
    assert_eq!(*status, ToolStatus::Ok);
    let TranscriptItem::Approval { tool, input, .. } = &items[5] else {
        panic!()
    };
    assert_eq!(tool, "Write");
    assert_eq!(input["file_path"], "/work/repo/b.txt");
    assert_eq!(
        items[6],
        TranscriptItem::Text {
            id: "msg_0ea25b3050922329016abea94b634887d294ade77e852f60eb".into(),
            text: "done".into()
        }
    );

    let meta = t.meta();
    assert!(!meta.busy);
    assert_eq!(t.active_turn(), None);
    assert_eq!(meta.context_tokens, Some(15391));
    assert_eq!(meta.context_window, Some(258400));
    assert_eq!(
        meta.title.as_deref(),
        Some("Ignore the image. Run exactly the shell command `echo hi > note.txt` (it needs w…")
    );
    assert_eq!(
        meta.usage_limits,
        Some(vec![
            UsageLimit {
                label: "session".into(),
                percent: 0,
                resets: None,
                resets_at: Some(1790897553)
            },
            UsageLimit {
                label: "week (all models)".into(),
                percent: 24,
                resets: None,
                resets_at: Some(1791292866)
            },
        ])
    );
    assert!(t.waiting_on().is_none());
}

#[test]
fn streams_text_in_place_and_tracks_busy() {
    let mut t = CodexTranscript::default();
    t.set_busy();
    t.apply(&note("turn/started", json!({"turn": {"id": "t1"}})));
    assert_eq!(t.active_turn(), Some("t1"));
    t.apply(&note(
        "item/started",
        json!({"item": {"type":"agentMessage","id":"m1","text":""}}),
    ));
    let a = t.apply(&note(
        "item/agentMessage/delta",
        json!({"itemId":"m1","delta":"Hel"}),
    ));
    assert_eq!(a.items, vec![0]);
    t.apply(&note(
        "item/agentMessage/delta",
        json!({"itemId":"m1","delta":"lo"}),
    ));
    assert_eq!(
        t.items()[0],
        TranscriptItem::Text {
            id: "m1".into(),
            text: "Hello".into()
        }
    );
    t.apply(&note(
        "item/completed",
        json!({"item": {"type":"agentMessage","id":"m1","text":"Hello."}}),
    ));
    assert_eq!(t.items().len(), 1, "the final text replaces, not appends");
    assert_eq!(
        t.items()[0],
        TranscriptItem::Text {
            id: "m1".into(),
            text: "Hello.".into()
        }
    );
    // Reasoning summaries stream into a thinking item, parts apart.
    t.apply(&note(
        "item/reasoning/summaryTextDelta",
        json!({"itemId":"r1","delta":"Plan","summaryIndex":0}),
    ));
    t.apply(&note(
        "item/reasoning/summaryPartAdded",
        json!({"itemId":"r1","summaryIndex":1}),
    ));
    t.apply(&note(
        "item/reasoning/summaryTextDelta",
        json!({"itemId":"r1","delta":"Act","summaryIndex":1}),
    ));
    assert_eq!(
        t.items()[1],
        TranscriptItem::Thinking {
            id: "r1".into(),
            text: "Plan\n\nAct".into()
        }
    );
    // An empty reasoning item adds nothing.
    t.apply(&note(
        "item/completed",
        json!({"item": {"type":"reasoning","id":"r2","summary":[],"content":[]}}),
    ));
    assert_eq!(t.items().len(), 2);
    assert!(t.meta().busy);
    let a = t.apply(&note(
        "turn/completed",
        json!({"turn": {"id":"t1","status":"interrupted"}}),
    ));
    assert!(a.meta);
    assert!(!t.meta().busy);
    assert_eq!(
        t.items()[2],
        TranscriptItem::Notice {
            id: "interrupted:t1".into(),
            text: "Interrupted".into()
        }
    );
}

#[test]
fn command_output_streams_and_long_output_is_previewed() {
    let mut t = CodexTranscript::default();
    t.apply(&note(
        "item/started",
        json!({"item": {"type":"commandExecution","id":"c1",
        "command":"/bin/bash -lc 'cargo test'","cwd":"/w","status":"inProgress"}}),
    ));
    t.apply(&note(
        "item/commandExecution/outputDelta",
        json!({"itemId":"c1","delta":"running 3 tests\n"}),
    ));
    let TranscriptItem::Tool {
        status,
        output,
        input,
        ..
    } = &t.items()[0]
    else {
        panic!()
    };
    assert_eq!(*status, ToolStatus::Running);
    assert_eq!(output.as_deref(), Some("running 3 tests\n"));
    assert_eq!(input["command"], "cargo test");
    assert_eq!(t.running_tool(), None, "idle: nothing counts as running");

    let long = "x".repeat(MAX_TEXT_BYTES + 10);
    t.apply(&note(
        "item/completed",
        json!({"item": {"type":"commandExecution","id":"c1",
        "command":"/bin/bash -lc 'cargo test'","cwd":"/w","status":"completed",
        "aggregatedOutput": long, "exitCode": 101}}),
    ));
    let TranscriptItem::Tool {
        status,
        output,
        full_output_bytes,
        ..
    } = &t.items()[0]
    else {
        panic!()
    };
    assert_eq!(*status, ToolStatus::Error, "a non-zero exit is an error");
    assert!(output.as_ref().unwrap().ends_with('…'));
    assert_eq!(*full_output_bytes, Some(long.len()));
    assert_eq!(t.full_output("c1"), Some(long.as_str()));
}

#[test]
fn file_updates_become_edits_with_hunks() {
    let mut t = CodexTranscript::default();
    t.apply(&note(
        "item/completed",
        json!({"item": {"type":"fileChange","id":"f1","status":"completed",
        "changes":[
            {"path":"/w/a.rs","kind":{"type":"update","move_path":null},
             "diff":"@@ -2,2 +2,3 @@\n use a;\n+use b;\n use c;\n@@ -9 +10 @@\n-old\n+new\n"},
            {"path":"/w/gone.rs","kind":{"type":"delete"},"diff":"bye\n"}
        ]}}),
    ));
    let TranscriptItem::Tool {
        id,
        name,
        input,
        patch,
        ..
    } = &t.items()[0]
    else {
        panic!()
    };
    assert_eq!((id.as_str(), name.as_str()), ("f1", "Edit"));
    assert_eq!(input, &json!({"file_path":"/w/a.rs"}));
    assert_eq!(
        patch,
        &Some(json!([
            {"oldStart":2,"newStart":2,"lines":[" use a;","+use b;"," use c;"]},
            {"oldStart":9,"newStart":10,"lines":["-old","+new"]},
        ]))
    );
    let TranscriptItem::Tool {
        id, input, patch, ..
    } = &t.items()[1]
    else {
        panic!()
    };
    assert_eq!(id, "f1:1");
    assert_eq!(input["deleted"], true);
    assert_eq!(
        patch,
        &Some(json!([{"oldStart":1,"newStart":0,"lines":["-bye"]}]))
    );
}

#[test]
fn deny_declines_and_interrupt_cancels_open_approvals() {
    let mut t = CodexTranscript::default();
    let ask = |id| {
        request(
            id,
            "item/commandExecution/requestApproval",
            json!({"itemId":"c1","command":"rm -rf build","cwd":"/w"}),
        )
    };
    t.apply(&ask(7));
    let (i, reply) = t
        .resolve_approval("request:7", ApprovalDecision::Deny, None)
        .unwrap();
    assert_eq!(
        reply,
        json!({"jsonrpc":"2.0","id":7,"result":{"decision":"decline"}})
    );
    assert!(
        t.resolve_approval("request:7", ApprovalDecision::Allow, None)
            .is_none(),
        "first answer wins"
    );
    let TranscriptItem::Approval { decision, .. } = &t.items()[i] else {
        panic!()
    };
    assert_eq!(*decision, Some(ApprovalDecision::Deny));

    // Offered only accept / amendment / cancel: deny falls back to cancel,
    // always-allow to a plain accept — never the amendment's lasting rule.
    let amendment = json!({"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["ls"]}});
    t.apply(&request(
        8,
        "item/commandExecution/requestApproval",
        json!({"command":"ls","availableDecisions":["accept", amendment, "cancel"]}),
    ));
    t.apply(&request(
        9,
        "item/commandExecution/requestApproval",
        json!({"command":"ls","availableDecisions":["accept", amendment, "cancel"]}),
    ));
    let (_, reply) = t
        .resolve_approval("request:8", ApprovalDecision::Deny, None)
        .unwrap();
    assert_eq!(reply["result"]["decision"], "cancel");
    let (_, reply) = t
        .resolve_approval("request:9", ApprovalDecision::AlwaysAllow, None)
        .unwrap();
    assert_eq!(reply["result"]["decision"], "accept");

    t.apply(&ask(10));
    t.apply(&request(
        11,
        "item/fileChange/requestApproval",
        json!({"itemId":"f9"}),
    ));
    let (changed, replies) = t.cancel_approvals();
    assert_eq!(changed.len(), 2);
    assert_eq!(replies.len(), 2);
    assert!(replies.iter().all(|r| r["result"]["decision"] == "cancel"));
    assert!(t.waiting_on().is_none());

    // Settled by codex itself: expired, never answered.
    t.apply(&ask(12));
    let a = t.apply(&note("serverRequest/resolved", json!({"requestId": 12})));
    let TranscriptItem::Approval { expired, .. } = &t.items()[a.items[0]] else {
        panic!()
    };
    assert!(expired);
    assert!(t
        .resolve_approval("request:12", ApprovalDecision::Allow, None)
        .is_none());
}

#[test]
fn questions_map_answers_back_to_their_ids() {
    let mut t = CodexTranscript::default();
    t.apply(&request(3, "item/tool/requestUserInput", json!({"itemId":"q","questions":[
        {"id":"lang","header":"Language","question":"Which language?","isOther":true,"isSecret":false,
         "options":[{"label":"Rust","description":"fast"},{"label":"Go","description":""}]},
        {"id":"name","header":"Name","question":"Project name?","isOther":false,"isSecret":false,"options":null}
    ]})));
    let TranscriptItem::Approval { tool, input, .. } = &t.items()[0] else {
        panic!()
    };
    assert_eq!(tool, "AskUserQuestion");
    assert_eq!(
        input["questions"][0],
        json!({"id":"lang","isOther":true,"isSecret":false,"question":"Which language?","header":"Language","multiSelect":false,
            "options":[{"label":"Rust","description":"fast"},{"label":"Go","description":""}]})
    );
    assert_eq!(input["questions"][1]["options"], json!([]));
    let answers = json!({"Which language?":"Rust","Project name?":"wb","Other":1});
    let (_, reply) = t
        .resolve_approval("request:3", ApprovalDecision::Allow, answers.as_object())
        .unwrap();
    assert_eq!(
        reply["result"],
        json!({"answers":{"lang":{"answers":["Rust"]},"name":{"answers":["wb"]}}})
    );
}

#[test]
fn permissions_grant_what_was_asked_or_nothing() {
    let mut t = CodexTranscript::default();
    let ask = |id| {
        request(
            id,
            "item/permissions/requestApproval",
            json!({"cwd":"/w","reason":"needs net",
        "permissions":{"network":{"enabled":true},"fileSystem":null}}),
        )
    };
    t.apply(&ask(1));
    t.apply(&ask(2));
    let (_, reply) = t
        .resolve_approval("request:1", ApprovalDecision::AlwaysAllow, None)
        .unwrap();
    assert_eq!(
        reply["result"],
        json!({"permissions":{"network":{"enabled":true}},"scope":"session"})
    );
    let (_, reply) = t
        .resolve_approval("request:2", ApprovalDecision::Deny, None)
        .unwrap();
    assert_eq!(reply["result"], json!({"permissions":{},"scope":"turn"}));
}

#[test]
fn unsupported_requests_are_declined_at_once() {
    let mut t = CodexTranscript::default();
    let a = t.apply(&request(4, "item/tool/call", json!({})));
    assert_eq!(a.reply.unwrap()["error"]["code"], -32601);
    let a = t.apply(&request(
        5,
        "mcpServer/elicitation/request",
        json!({"mode":"openai/userVerification"}),
    ));
    assert_eq!(a.reply.unwrap()["result"]["action"], "decline");
    assert!(matches!(t.items()[0], TranscriptItem::Notice { .. }));
    let a = t.apply(&note("some/new/thing", json!({})));
    assert_eq!(a.unknown_method.as_deref(), Some("some/new/thing"));
    assert_eq!(
        t.apply(&note("some/new/thing", json!({}))).unknown_method,
        None
    );
}

#[test]
fn errors_retry_quietly_then_show_once() {
    let mut t = CodexTranscript::default();
    t.apply(&note("turn/started", json!({"turn": {"id": "t1"}})));
    t.apply(&note(
        "error",
        json!({"error":{"message":"Reconnecting... 2/5"},"willRetry":true,"turnId":"t1"}),
    ));
    let retry = t.meta().retry.clone().unwrap();
    assert_eq!((retry.attempt, retry.max_retries), (2, 5));
    t.apply(&note(
        "error",
        json!({"error":{"message":"routing failed"},"willRetry":false,"turnId":"t1"}),
    ));
    t.apply(&note(
        "turn/completed",
        json!({"turn":{"id":"t1","status":"failed",
        "error":{"message":"routing failed"}}}),
    ));
    assert_eq!(
        t.items(),
        &[TranscriptItem::Notice {
            id: "turn-error:t1".into(),
            text: "routing failed".into()
        }]
    );
    assert_eq!(t.meta().retry, None);
}

#[test]
fn plan_updates_become_one_todo_list_per_turn() {
    let mut t = CodexTranscript::default();
    for status in ["inProgress", "completed"] {
        t.apply(&note("turn/plan/updated", json!({"turnId":"t1","plan":[
            {"step":"Read","status":"completed"},{"step":"Fix","status":status},{"step":"Test","status":"pending"}]})));
    }
    assert_eq!(t.items().len(), 1);
    let TranscriptItem::Tool { name, input, .. } = &t.items()[0] else {
        panic!()
    };
    assert_eq!(name, "TodoWrite");
    assert_eq!(
        input["todos"],
        json!([{"content":"Read","status":"completed"},{"content":"Fix","status":"completed"},
            {"content":"Test","status":"pending"}])
    );
}

#[test]
fn thread_result_sets_model_preset_and_title() {
    let mut t = CodexTranscript::default();
    t.apply_thread(&json!({"model":"gpt-6.1-sol","approvalPolicy":"on-request",
        "sandbox":{"type":"workspaceWrite"},"thread":{"name":null,"preview":"Fix the build\nplease"}}));
    assert_eq!(t.meta().model.as_deref(), Some("gpt-6.1-sol"));
    assert_eq!(t.meta().permission_mode.as_deref(), Some("auto"));
    assert_eq!(t.meta().title.as_deref(), Some("Fix the build"));
    t.apply_thread(
        &json!({"approvalPolicy":{"granular":{}},"sandbox":{"type":"readOnly"},"thread":{}}),
    );
    assert_eq!(
        t.meta().permission_mode,
        None,
        "a custom policy is no preset"
    );
    t.apply(&note(
        "thread/name/updated",
        json!({"threadName":"Build fix"}),
    ));
    assert_eq!(t.meta().title.as_deref(), Some("Build fix"));

    let models = json!([{"id":"gpt-a","model":"gpt-a","displayName":"A","description":"d","hidden":false,
        "supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"high"}]},
        {"id":"secret","model":"secret","hidden":true}]);
    t.set_models(models.as_array().unwrap());
    assert_eq!(t.meta().models.len(), 1);
    assert_eq!(t.meta().models[0].effort_levels, ["low", "high"]);
    t.set_model_choice("gpt-a");
    assert_eq!(t.meta().model.as_deref(), Some("gpt-a"));
}

#[test]
fn history_loads_settled() {
    let mut entries = Vec::new();
    for line in FIXTURE.lines() {
        let msg: Value = serde_json::from_str(line).unwrap();
        if msg["method"] == "item/completed" {
            entries.push(json!({"item": msg["params"]["item"], "startedAtMs": 1}));
        }
    }
    let mut t = CodexTranscript::default();
    t.load_history(&entries);
    assert_eq!(t.items().len(), 5);
    assert!(!t.meta().busy);
    assert!(t.items().iter().all(|i| !matches!(
        i,
        TranscriptItem::Tool {
            status: ToolStatus::Running,
            ..
        }
    )));
}

#[test]
fn shell_wrappers_are_stripped_only_when_unambiguous() {
    assert_eq!(
        strip_shell("/bin/zsh -lc 'echo hi > note.txt'"),
        "echo hi > note.txt"
    );
    assert_eq!(strip_shell("bash -lc 'echo '\\''x'\\'''"), "echo 'x'");
    assert_eq!(strip_shell("/bin/zsh -lc ls"), "ls");
    assert_eq!(
        strip_shell("/bin/zsh -lc 'a' && 'b'"),
        "/bin/zsh -lc 'a' && 'b'"
    );
    assert_eq!(
        strip_shell("git -c core.x=1 status"),
        "git -c core.x=1 status"
    );
    assert_eq!(iso_utc(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(iso_utc(951_782_400_000), "2000-02-29T00:00:00.000Z");
}

#[test]
fn presets_round_trip() {
    for mode in CODEX_MODES {
        let (policy, _) = sandbox_mode(mode).unwrap();
        let sandbox = sandbox_policy(mode).unwrap();
        assert_eq!(
            mode_of(&json!(policy), sandbox["type"].as_str().unwrap()),
            Some(*mode)
        );
    }
    assert!(sandbox_mode("bypassPermissions").is_none());
}

#[test]
fn network_approval_preserves_destination_without_inventing_a_command() {
    let mut t = CodexTranscript::default();
    t.apply(&request(1,"item/commandExecution/requestApproval",json!({"networkApprovalContext":{"host":"example.com","protocol":"https"},"availableDecisions":["accept","decline"],"environment":"container"})));
    let TranscriptItem::Approval {
        tool,
        input,
        can_always_allow,
        ..
    } = &t.items()[0]
    else {
        panic!()
    };
    assert_eq!(tool, "Network");
    assert_eq!(input["networkApprovalContext"]["host"], "example.com");
    assert_eq!(input["environment"], "container");
    assert!(!can_always_allow);
    let (_, reply) = t
        .resolve_approval("request:1", ApprovalDecision::AlwaysAllow, None)
        .unwrap();
    assert_eq!(reply["result"]["decision"], "accept");
}

#[test]
fn duplicate_questions_are_answered_by_id_and_secrets_never_echo_in_transcript() {
    let mut t = CodexTranscript::default();
    t.apply(&request(
        1,
        "item/tool/requestUserInput",
        json!({"isBlocking":false,"questions":[
            {"id":"first","question":"Which value?","isOther":false,"isSecret":true,"options":null},
            {"id":"second","question":"Which value?","isOther":true,"isSecret":false,"options":null}
        ]}),
    ));
    assert!(t.waiting_on().is_none());
    let (_, reply) = t
        .resolve_approval(
            "request:1",
            ApprovalDecision::Allow,
            json!({"first":"secret-value","second":"public-value"}).as_object(),
        )
        .unwrap();
    assert_eq!(
        reply["result"]["answers"]["first"]["answers"],
        json!(["secret-value"])
    );
    assert_eq!(
        reply["result"]["answers"]["second"]["answers"],
        json!(["public-value"])
    );
    assert!(!serde_json::to_string(t.items())
        .unwrap()
        .contains("secret-value"));
}

#[test]
fn elicitation_invalid_form_can_be_corrected_and_first_valid_answer_wins() {
    let mut t = CodexTranscript::default();
    t.apply(&request(1,"mcpServer/elicitation/request",json!({"mode":"form","serverName":"example","requestedSchema":{"type":"object","required":["count","choice"],"properties":{"count":{"type":"integer","minimum":1,"maximum":5},"choice":{"type":"string","enum":["A","B"]}}}})));
    assert!(t
        .resolve_elicitation_checked(
            "request:1",
            "accept",
            Some(&json!({"count":0,"choice":"A"}))
        )
        .is_err());
    assert!(t.waiting_on().is_some());
    let (_, reply) = t
        .resolve_elicitation_checked(
            "request:1",
            "accept",
            Some(&json!({"count":2,"choice":"B"})),
        )
        .unwrap();
    assert_eq!(reply["result"]["action"], "accept");
    assert_eq!(reply["result"]["content"], json!({"count":2,"choice":"B"}));
    assert!(t
        .resolve_elicitation_checked("request:1", "decline", None)
        .is_err());
    // Form responses are not copied into the transcript, which can contain passwords.
    let TranscriptItem::Elicitation { content, .. } = &t.items()[0] else {
        panic!()
    };
    assert!(content.is_none());
    t.apply(&request(
        2,
        "mcpServer/elicitation/request",
        json!({"mode":"url","url":"https://example.com"}),
    ));
    let (_, replies) = t.cancel_approvals();
    assert_eq!(replies[0]["result"]["action"], "cancel");
}

#[test]
fn subagents_review_artifacts_and_unknown_items_are_visible() {
    let mut t = CodexTranscript::default();
    for item in [
        json!({"type":"collabAgentToolCall","id":"spawn","tool":"spawnAgent","receiverThreadIds":["child"],"agentsStates":{"child":{"status":"running","message":null}},"prompt":"Run tests","status":"completed"}),
        json!({"type":"subAgentActivity","id":"done","agentThreadId":"child","agentPath":"tests","kind":"completed"}),
        json!({"type":"enteredReviewMode","id":"review","review":"Reviewing changes"}),
        json!({"type":"imageGeneration","id":"image","result":"aGVsbG8=","status":"completed"}),
        json!({"type":"futureKind","id":"future","description":"New activity"}),
    ] {
        let fx = t.apply(&note("item/completed", json!({"item":item})));
        assert!(!fx.items.is_empty());
    }
    assert_eq!(t.meta().tasks[0].id, "child");
    assert_eq!(t.meta().tasks[0].status, "completed");
    assert_eq!(t.artifacts("image").unwrap()[0]["mimeType"], "image/png");
    assert!(t
        .items()
        .iter()
        .any(|i| matches!(i,TranscriptItem::Notice{text,..}if text.contains("futureKind"))));
}

#[test]
fn model_catalog_and_effective_thread_settings_keep_defaults_and_tiers() {
    let mut t = CodexTranscript::default();
    t.apply_thread(&json!({"model":"one","reasoningEffort":"high","approvalPolicy":"never","sandbox":{"type":"readOnly"},"thread":{"id":"t"}}));
    t.set_models(&[json!({"id":"one","displayName":"One","defaultReasoningEffort":"medium","inputModalities":["text","image"],"serviceTiers":[{"id":"fast","name":"Fast","description":"Faster"}]})]);
    assert_eq!(t.meta().effort.as_deref(), Some("high"));
    assert_eq!(
        t.meta().codex.as_ref().unwrap().approval_policy,
        Some(json!("never"))
    );
    assert_eq!(t.meta().models[0].default_effort.as_deref(), Some("medium"));
    assert_eq!(t.meta().models[0].service_tiers[0]["id"], "fast");
}

#[test]
fn mcp_resources_and_structured_results_are_preserved_as_output() {
    let mut t = CodexTranscript::default();
    t.apply(&note("item/completed",json!({"item":{"id":"resource","type":"mcpToolCall","server":"example","tool":"read","status":"completed","arguments":{},"result":{"content":[{"type":"resource","resource":{"uri":"test://readme","text":"Embedded README"}},{"type":"resource_link","uri":"test://guide","name":"Guide"}],"structuredContent":{"answer":42}}}})));
    let output = serde_json::to_value(t.items()).unwrap().to_string();
    assert!(output.contains("Embedded README"));
    assert!(output.contains("test://guide"));
    assert!(output.contains("answer"));
}
