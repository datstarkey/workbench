use super::parse::MAX_TEXT_BYTES;
use super::*;
use serde_json::json;
use std::fs;

const SID: &str = "7b3c54f4-ba22-4654-9af9-037d1cd8e555";

fn user(uuid: &str, content: Value) -> Value {
    json!({"type":"user","uuid":uuid,"timestamp":"2026-09-30T21:07:10Z","message":{"role":"user","content":content}})
}

fn assistant(uuid: &str, msg_id: &str, block: Value) -> Value {
    json!({"type":"assistant","uuid":uuid,"message":{"id":msg_id,"model":"claude-opus-5-5","content":[block],
        "usage":{"input_tokens":2,"cache_read_input_tokens":100,"cache_creation_input_tokens":50}}})
}

fn stream(event: Value) -> Value {
    json!({"type":"stream_event","event":event,"parent_tool_use_id":null})
}

fn user_texts(t: &Transcript) -> Vec<&str> {
    t.items()
        .iter()
        .filter_map(|i| match i {
            TranscriptItem::User { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn folds_a_jsonl_turn_into_chat_items() {
    let mut t = Transcript::default();
    t.apply(&user("u1", json!("Fix the keyboard inset")));
    assert!(t.meta().busy);
    t.apply(&assistant(
        "a1",
        "m1",
        json!({"type":"text","text":"On it."}),
    ));
    t.apply(&assistant(
        "a2",
        "m1",
        json!({"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"bun test"}}),
    ));
    let applied = t.apply(&json!({"type":"user","uuid":"u2","message":{"content":[
        {"type":"tool_result","tool_use_id":"toolu_1","content":"41 passed","is_error":false}]}}));
    assert_eq!(applied.items, vec![2]);
    t.apply(&json!({"type":"system","subtype":"turn_duration","durationMs":10}));

    assert_eq!(t.items().len(), 3);
    assert!(matches!(&t.items()[1], TranscriptItem::Text { text, .. } if text == "On it."));
    assert!(
        matches!(&t.items()[2], TranscriptItem::Tool { status: ToolStatus::Ok, output: Some(o), .. } if o == "41 passed")
    );
    assert_eq!(t.meta().model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(t.meta().context_tokens, Some(152));
    assert!(!t.meta().busy);
}

#[test]
fn edit_results_carry_the_patch_in_either_spelling() {
    for key in ["toolUseResult", "tool_use_result"] {
        let mut t = Transcript::default();
        t.apply(&assistant(
            "a1",
            "m1",
            json!({"type":"tool_use","id":"toolu_e","name":"Edit","input":{"file_path":"/x.ts"}}),
        ));
        let mut line = json!({"type":"user","uuid":"u","message":{"content":[
            {"type":"tool_result","tool_use_id":"toolu_e","content":"ok"}]}});
        line[key] = json!({"structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-a","+b"]}]});
        t.apply(&line);
        let TranscriptItem::Tool { patch: Some(p), .. } = &t.items()[0] else {
            panic!("expected patch for {key}");
        };
        assert_eq!(p[0]["lines"], json!(["-a", "+b"]));
    }
}

#[test]
fn streamed_text_is_replaced_in_place_by_the_final_block() {
    let mut t = Transcript::default();
    t.apply(&stream(
        json!({"type":"message_start","message":{"id":"m1","model":"claude-opus-5-5"}}),
    ));
    assert!(t.meta().busy);
    t.apply(&stream(
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
    ));
    t.apply(&stream(
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}),
    ));
    let a = t.apply(&stream(
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}),
    ));
    assert_eq!(a.items, vec![0]);
    assert!(
        matches!(&t.items()[0], TranscriptItem::Text { id, text } if id == "m1:0" && text == "Hello")
    );

    t.apply(&assistant(
        "a1",
        "m1",
        json!({"type":"text","text":"Hello there."}),
    ));
    assert_eq!(t.items().len(), 1, "final block must not add a second item");
    assert!(
        matches!(&t.items()[0], TranscriptItem::Text { id, text } if id == "m1:0" && text == "Hello there.")
    );

    t.apply(&json!({"type":"result","subtype":"success","is_error":false}));
    assert!(!t.meta().busy);
}

#[test]
fn a_streamed_tool_call_shows_running_then_gets_its_input() {
    let mut t = Transcript::default();
    t.apply(&stream(
        json!({"type":"message_start","message":{"id":"m1"}}),
    ));
    t.apply(&stream(json!({"type":"content_block_start","index":0,
        "content_block":{"type":"tool_use","id":"toolu_9","name":"Bash","input":{}}})));
    assert!(
        matches!(&t.items()[0], TranscriptItem::Tool { name, status: ToolStatus::Running, .. } if name == "Bash")
    );
    t.apply(&assistant(
        "a1",
        "m1",
        json!({"type":"tool_use","id":"toolu_9","name":"Bash","input":{"command":"echo hi"}}),
    ));
    assert_eq!(t.items().len(), 1);
    assert!(
        matches!(&t.items()[0], TranscriptItem::Tool { input, .. } if input["command"] == "echo hi")
    );
}

#[test]
fn approvals_round_trip_to_control_responses() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"control_request","request_id":"req-1","request":{
        "subtype":"can_use_tool","tool_name":"Bash","input":{"command":"touch x"},
        "description":"Create x","blocked_path":"/repo/x",
        "permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"touch x"}],"behavior":"allow","destination":"localSettings"}]}}));
    assert!(matches!(
        &t.items()[0],
        TranscriptItem::Approval { tool, can_always_allow: true, decision: None, .. } if tool == "Bash"
    ));
    assert_eq!(t.pending_approval_ids(), vec!["req-1".to_string()]);

    let (i, response) = t
        .resolve_approval("req-1", ApprovalDecision::AlwaysAllow, None)
        .expect("pending approval");
    assert_eq!(i, 0);
    assert_eq!(response["type"], "control_response");
    assert_eq!(response["response"]["request_id"], "req-1");
    let body = &response["response"]["response"];
    assert_eq!(body["behavior"], "allow");
    assert_eq!(body["updatedInput"]["command"], "touch x");
    assert_eq!(body["updatedPermissions"][0]["type"], "addRules");
    assert!(matches!(
        &t.items()[0],
        TranscriptItem::Approval {
            decision: Some(ApprovalDecision::AlwaysAllow),
            ..
        }
    ));
    assert!(
        t.resolve_approval("req-1", ApprovalDecision::Deny, None)
            .is_none(),
        "answered once"
    );
}

#[test]
fn deny_tells_claude_why() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Write","input":{}}}));
    let (_, response) = t
        .resolve_approval("r", ApprovalDecision::Deny, None)
        .unwrap();
    assert_eq!(response["response"]["response"]["behavior"], "deny");
    assert!(response["response"]["response"]["message"]
        .as_str()
        .is_some());
}

#[test]
fn init_and_errors_update_the_chat() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5[1m]","permissionMode":"default"}));
    assert_eq!(t.meta().permission_mode.as_deref(), Some("default"));
    t.set_busy();
    t.apply(
        &json!({"type":"result","subtype":"error_during_execution","is_error":true,"uuid":"r1"}),
    );
    assert!(!t.meta().busy);
    assert!(matches!(&t.items()[0], TranscriptItem::Notice { .. }));
}

#[test]
fn subagent_events_are_left_to_their_task_card() {
    let mut t = Transcript::default();
    let mut line = assistant("a", "m", json!({"type":"text","text":"inside a subagent"}));
    line["parent_tool_use_id"] = json!("toolu_task");
    t.apply(&line);
    t.apply(&json!({"type":"user","uuid":"side","isSidechain":true,"message":{"content":"subagent prompt"}}));
    assert!(t.items().is_empty());
}

#[test]
fn hides_bookkeeping_and_shows_slash_commands_and_html() {
    let mut t = Transcript::default();
    t.apply(
        &json!({"type":"user","uuid":"m","isMeta":true,"message":{"content":"Base directory…"}}),
    );
    t.apply(&user(
        "s",
        json!("<local-command-stdout>done</local-command-stdout>"),
    ));
    t.apply(&user(
        "sc",
        json!("<command-name>/compact</command-name>\n<command-args>keep tests</command-args>"),
    ));
    t.apply(&assistant(
        "th",
        "m",
        json!({"type":"thinking","thinking":""}),
    ));
    t.apply(&user("ok", json!("ok")));
    t.apply(&user("html", json!("<Button> renders twice, why?")));
    assert_eq!(
        user_texts(&t),
        vec!["/compact keep tests", "ok", "<Button> renders twice, why?"]
    );
}

#[test]
fn slash_commands_do_not_start_a_turn() {
    let mut t = Transcript::default();
    t.apply(&user("c", json!("<command-name>/cost</command-name>")));
    assert!(!t.meta().busy);
    t.apply(&user("p", json!("fix it")));
    assert!(t.meta().busy);
}

#[test]
fn mid_turn_prompts_are_shown() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"attachment","uuid":"q1","attachment":{
        "type":"queued_command","prompt":"also check windows ","commandMode":"prompt","timestamp":"t"}}));
    t.apply(&json!({"type":"attachment","uuid":"q2","attachment":{"type":"file","prompt":"x"}}));
    assert_eq!(user_texts(&t), vec!["also check windows"]);
}

#[test]
fn interrupt_ends_the_turn() {
    let mut t = Transcript::default();
    t.apply(&user("u1", json!("go")));
    t.apply(&user(
        "u2",
        json!([{"type":"text","text":"[Request interrupted by user]"}]),
    ));
    assert!(!t.meta().busy);
    assert!(matches!(&t.items()[1], TranscriptItem::Notice { .. }));
}

#[test]
fn long_tool_input_is_clipped() {
    let mut t = Transcript::default();
    let body = "x".repeat(MAX_TEXT_BYTES * 2);
    t.apply(&assistant(
        "a",
        "m",
        json!({"type":"tool_use","id":"toolu_w","name":"Write","input":{"content":body}}),
    ));
    let TranscriptItem::Tool { input, .. } = &t.items()[0] else {
        panic!();
    };
    assert!(input["content"].as_str().unwrap().len() <= MAX_TEXT_BYTES + 3);
}

#[test]
fn load_reads_history_and_never_reports_a_turn_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("-repo");
    fs::create_dir_all(&project).unwrap();
    let path = project.join(format!("{SID}.jsonl"));
    fs::write(
        &path,
        format!(
            "{}\nnot json\n{}\n",
            user("u1", json!("hello there")),
            assistant("a1", "m1", json!({"type":"text","text":"hi"}))
        ),
    )
    .unwrap();
    assert_eq!(find_transcript(dir.path(), SID), Some(path.clone()));
    let t = Transcript::load(&path);
    assert_eq!(t.items().len(), 2);
    assert!(!t.meta().busy);
    assert!(Transcript::load(&dir.path().join("missing.jsonl"))
        .items()
        .is_empty());
}

#[test]
fn uuid_check_rejects_paths() {
    assert!(is_uuid(SID));
    assert!(!is_uuid("../../etc/passwd"));
    assert!(!is_uuid("7b3c54f4-ba22-4654-9af9-037d1cd8e55"));
    assert!(!is_uuid("7b3c54f4/ba22-4654-9af9-037d1cd8e555"));
}

#[test]
fn a_recorded_cli_turn_has_no_unknown_kinds() {
    // A real `claude -p` stream-json turn (CLI 2.1.286) that asked for and got
    // permission to run a Bash command.
    let mut t = Transcript::default();
    let mut unknown = Vec::new();
    for line in include_str!("fixtures/stream-2.1.286.jsonl").lines() {
        unknown.extend(t.apply_line(line).unknown_kind);
    }
    assert!(unknown.is_empty(), "add these to protocol.rs: {unknown:?}");
    assert!(t.items().iter().any(|i| matches!(i,
        TranscriptItem::Approval { tool, .. } if tool == "Bash")));
    assert!(t.items().iter().any(|i| matches!(i,
        TranscriptItem::Tool { name, status: ToolStatus::Ok, .. } if name == "Bash")));
    assert!(!t.meta().busy, "the result line ends the turn");
}

#[test]
fn unknown_kinds_are_reported_once() {
    let mut t = Transcript::default();
    let line = json!({"type":"system","subtype":"brand_new_thing"});
    assert_eq!(
        t.apply(&line).unknown_kind.as_deref(),
        Some("system:brand_new_thing")
    );
    assert_eq!(t.apply(&line).unknown_kind, None);
}

#[test]
fn a_withdrawn_approval_expires_and_cannot_be_answered() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{}}}));
    let a = t.apply(&json!({"type":"control_cancel_request","request_id":"r"}));
    assert_eq!(a.items, vec![0]);
    assert!(matches!(
        &t.items()[0],
        TranscriptItem::Approval { expired: true, .. }
    ));
    assert!(t
        .resolve_approval("r", ApprovalDecision::Allow, None)
        .is_none());
}

#[test]
fn unsupported_host_requests_get_an_error_reply() {
    let mut t = Transcript::default();
    let a = t.apply(&json!({"type":"control_request","request_id":"e1","request":{"subtype":"elicitation","message":"Sign in?"}}));
    let reply = a.reply.expect("the CLI must not be left waiting");
    assert_eq!(reply["response"]["subtype"], "error");
    assert_eq!(reply["response"]["request_id"], "e1");
    assert!(t.items().is_empty());
}

#[test]
fn clear_starts_over_under_the_new_session_id() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"init","model":"m","permissionMode":"plan"}));
    t.apply(&user("u", json!("hello")));
    let a = t.apply(&json!({"type":"conversation_reset","new_conversation_id":SID,"uuid":"x","session_id":"old"}));
    assert_eq!(a.new_session_id.as_deref(), Some(SID));
    assert!(t.items().is_empty());
    assert_eq!(
        t.meta().permission_mode.as_deref(),
        Some("plan"),
        "mode survives /clear"
    );
}

#[test]
fn slash_command_output_is_shown() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"local_command_output","content":"Total cost: $0.42","uuid":"o1"}));
    assert!(matches!(&t.items()[0], TranscriptItem::Notice { text, .. } if text.contains("$0.42")));
}

#[test]
fn questions_are_answered_through_updated_input() {
    let mut t = Transcript::default();
    let question = "Which library should we use?";
    t.apply(&json!({"type":"control_request","request_id":"q","request":{
        "subtype":"can_use_tool","tool_name":"AskUserQuestion",
        "input":{"questions":[{"question":question,"header":"Library","multiSelect":false,
            "options":[{"label":"date-fns","description":"Small"},{"label":"dayjs","description":"Tiny"}]}]}}}));
    let mut answers = serde_json::Map::new();
    answers.insert(question.into(), json!("dayjs"));
    answers.insert("ignored".into(), json!(42));
    let (_, response) = t
        .resolve_approval("q", ApprovalDecision::Allow, Some(&answers))
        .unwrap();
    let input = &response["response"]["response"]["updatedInput"];
    assert_eq!(
        input["answers"],
        json!({question: "dayjs"}),
        "non-strings dropped"
    );
    assert_eq!(
        input["questions"][0]["header"], "Library",
        "the questions go back unchanged"
    );
    assert!(matches!(&t.items()[0],
        TranscriptItem::Approval { answers: Some(a), .. } if a[question] == "dayjs"));
}

#[test]
fn subagents_and_background_jobs_are_tracked() {
    let mut t = Transcript::default();
    let sys = |sub: &str, extra: Value| {
        let mut v = json!({"type":"system","subtype":sub,"uuid":"x","session_id":"s"});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    };
    t.apply(&sys(
        "task_started",
        json!({"task_id":"a1","tool_use_id":"toolu_task",
        "description":"Find flaky tests","subagent_type":"Explore","task_type":"local_agent"}),
    ));
    let a = t.apply(&sys(
        "task_progress",
        json!({"task_id":"a1","description":"Find flaky tests",
        "usage":{"total_tokens":1800,"tool_uses":7,"duration_ms":12000},"last_tool_name":"Grep"}),
    ));
    assert!(a.meta, "progress reaches clients through meta");
    t.apply(&sys(
        "background_tasks_changed",
        json!({"tasks":[
        {"task_id":"b1","task_type":"local_bash","description":"bun run dev"}]}),
    ));
    t.apply(&sys(
        "task_notification",
        json!({"task_id":"a1","status":"completed",
        "output_file":"/tmp/o","summary":"Two tests depend on wall-clock time."}),
    ));
    t.apply(&sys(
        "task_updated",
        json!({"task_id":"b1","patch":{"status":"failed","error":"exit 1"}}),
    ));

    let tasks = &t.meta().tasks;
    assert_eq!(tasks.len(), 2);
    let agent = &tasks[0];
    assert_eq!(
        (agent.kind.as_str(), agent.status.as_str()),
        ("agent", "completed")
    );
    assert_eq!(agent.subagent_type.as_deref(), Some("Explore"));
    assert_eq!((agent.tool_uses, agent.tokens), (7, 1800));
    assert_eq!(agent.last_tool.as_deref(), Some("Grep"));
    assert_eq!(
        agent.description, "Find flaky tests",
        "progress keeps the task's name"
    );
    assert_eq!(agent.activity, None, "a finished task has no current step");
    assert!(agent.summary.as_deref().unwrap().contains("wall-clock"));
    let shell = &tasks[1];
    assert_eq!(
        (shell.kind.as_str(), shell.status.as_str()),
        ("local_bash", "failed")
    );
    assert!(shell.background);
    assert_eq!(shell.description, "bun run dev");
}

#[test]
fn retries_and_usage_limits_reach_the_chat() {
    let mut t = Transcript::default();
    t.apply(
        &json!({"type":"system","subtype":"api_retry","attempt":2,"max_retries":10,
        "retry_delay_ms":4000,"error_status":529,"error":"overloaded","uuid":"r","session_id":"s"}),
    );
    let retry = t.meta().retry.clone().expect("retry shown");
    assert_eq!(
        (retry.attempt, retry.max_retries, retry.error.as_deref()),
        (2, 10, Some("overloaded"))
    );
    t.apply(&stream(
        json!({"type":"message_start","message":{"id":"m"}}),
    ));
    assert!(t.meta().retry.is_none(), "cleared once the model answers");

    t.apply(
        &json!({"type":"rate_limit_event","rate_limit_info":{"status":"rejected",
        "resetsAt":1790857800,"rateLimitType":"five_hour","utilization":1.0}}),
    );
    let limit = t.meta().rate_limit.clone().unwrap();
    assert_eq!(limit.status, "rejected");
    assert_eq!(limit.resets_at, Some(1790857800));
    assert_eq!(limit.kind.as_deref(), Some("five_hour"));
}

#[test]
fn long_tool_output_keeps_a_preview_and_the_whole_text() {
    let mut t = Transcript::default();
    t.apply(&assistant(
        "a",
        "m",
        json!({"type":"tool_use","id":"toolu_l","name":"Bash","input":{}}),
    ));
    let long = "line\n".repeat(3000);
    t.apply(&json!({"type":"user","uuid":"u","message":{"content":[
        {"type":"tool_result","tool_use_id":"toolu_l","content":long}]}}));
    let TranscriptItem::Tool {
        output: Some(preview),
        full_output_bytes: Some(bytes),
        ..
    } = &t.items()[0]
    else {
        panic!("expected a preview");
    };
    assert!(preview.len() <= MAX_TEXT_BYTES + 3);
    assert_eq!(*bytes, long.len());
    assert_eq!(t.full_output("toolu_l"), Some(long.as_str()));
}
