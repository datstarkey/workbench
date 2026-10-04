//! MCP elicitation: an MCP server asking the person for input mid-turn (MCP
//! `elicitation/create`). Claude forwards it as an `elicitation` control
//! request, Codex as `mcpServer/elicitation/request`; both become an
//! [`TranscriptItem::Elicitation`] answered with an MCP `ElicitResult`.
//!
//! Form mode carries a flat JSON schema of primitive fields; URL mode asks the
//! person to open a page (OAuth, payment) and later reports completion.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{clip, clip_value, TranscriptItem};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ElicitationAction {
    Accept,
    Decline,
    Cancel,
}

/// What the CLI forwarded, in its own field names.
pub(crate) struct Request<'a> {
    pub id: String,
    pub server: &'a str,
    pub message: &'a str,
    pub mode: Option<&'a str>,
    pub url: Option<&'a str>,
    pub elicitation_id: Option<&'a str>,
    pub schema: Option<&'a Value>,
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
}

/// An elicitation waiting for an answer.
#[derive(Debug)]
pub(crate) struct Pending {
    pub item: usize,
    /// The whole schema (the item's copy is clipped), to check answers against.
    schema: Option<Value>,
}

impl Request<'_> {
    pub fn into_item(self) -> (TranscriptItem, Option<Value>) {
        let url_mode = self.mode == Some("url");
        let schema = self.schema.filter(|_| !url_mode).cloned();
        let item = TranscriptItem::Elicitation {
            id: self.id,
            server: self.server.to_string(),
            message: clip(self.message),
            mode: if url_mode { "url" } else { "form" }.to_string(),
            url: self.url.filter(|_| url_mode).map(String::from),
            elicitation_id: self.elicitation_id.map(String::from),
            schema: schema.as_ref().map(clip_value),
            title: self.title.map(String::from),
            description: self.description.map(String::from),
            expired: false,
            completed: false,
            action: None,
            content: None,
        };
        (item, schema)
    }
}

impl Pending {
    pub fn new(item: usize, schema: Option<Value>) -> Self {
        Self { item, schema }
    }

    /// Record the answer on its item and build the MCP `ElicitResult`. Only an
    /// accepted form sends content, and only the fields its schema declares.
    pub fn answer(
        self,
        items: &mut [TranscriptItem],
        action: ElicitationAction,
        content: Option<&Map<String, Value>>,
    ) -> Value {
        let content = (action == ElicitationAction::Accept)
            .then(|| self.schema.as_ref().map(|s| sanitize(s, content)))
            .flatten();
        if let TranscriptItem::Elicitation {
            action: a,
            content: c,
            ..
        } = &mut items[self.item]
        {
            *a = Some(action);
            *c = content.clone().map(Value::Object);
        }
        match content {
            Some(content) => json!({"action": action, "content": content}),
            None => json!({"action": action}),
        }
    }
}

/// Mark a pending elicitation withdrawn; true if `i` was one.
pub(crate) fn expire(items: &mut [TranscriptItem], i: usize) -> bool {
    match &mut items[i] {
        TranscriptItem::Elicitation { expired, .. } => {
            *expired = true;
            true
        }
        _ => false,
    }
}

/// The URL-mode elicitation the server says is done; its index if found.
pub(crate) fn complete(items: &mut [TranscriptItem], server: &str, id: &str) -> Option<usize> {
    items.iter_mut().rposition(|item| match item {
        TranscriptItem::Elicitation {
            server: s,
            elicitation_id: Some(e),
            completed,
            ..
        } if s == server && e == id => {
            *completed = true;
            true
        }
        _ => false,
    })
}

/// The answers the schema declares, each of its declared type. A client
/// can send anything; the MCP server only gets what it asked for.
fn sanitize(schema: &Value, content: Option<&Map<String, Value>>) -> Map<String, Value> {
    let (Some(props), Some(content)) =
        (schema.get("properties").and_then(Value::as_object), content)
    else {
        return Map::new();
    };
    content
        .iter()
        .filter(|(k, v)| props.get(*k).is_some_and(|p| fits(p, v)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn fits(prop: &Value, v: &Value) -> bool {
    match prop.get("type").and_then(Value::as_str) {
        Some("string") => v.is_string(),
        Some("number") => v.is_number(),
        Some("integer") => v.is_i64() || v.is_u64(),
        Some("boolean") => v.is_boolean(),
        Some("array") => v.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ChatView, Transcript};
    use super::*;

    fn request(id: &str, request: Value) -> Value {
        json!({"type":"control_request","request_id":id,"request":request})
    }

    #[test]
    fn form_elicitations_answer_with_only_the_declared_fields() {
        let mut t = Transcript::default();
        let a = t.apply(&request(
            "el-1",
            json!({
            "subtype":"elicitation","mcp_server_name":"deploy","message":"Which environment?",
            "mode":"form","requested_schema":{"type":"object","properties":{
                "env":{"type":"string","enum":["staging","prod"]},
                "replicas":{"type":"integer"},
                "confirm":{"type":"boolean"}},"required":["env"]}}),
        ));
        assert!(a.reply.is_none(), "waits for the person");
        assert!(matches!(
            &t.items()[0],
            TranscriptItem::Elicitation { server, mode, schema: Some(_), action: None, .. }
                if server == "deploy" && mode == "form"
        ));
        let w = t.waiting_on().unwrap().waiting_summary().unwrap();
        assert_eq!(w.id, "el-1");
        assert_eq!(w.tool, "Elicitation");
        assert_eq!(w.preview, "deploy: Which environment?");

        let content = json!({"env":"prod","replicas":2.5,"confirm":true,"extra":"x"});
        let (i, response) = t
            .resolve_elicitation("el-1", ElicitationAction::Accept, content.as_object())
            .expect("pending elicitation");
        assert_eq!(
            response,
            json!({"type":"control_response","response":{"subtype":"success","request_id":"el-1",
                "response":{"action":"accept","content":{"env":"prod","confirm":true}}}})
        );
        assert!(matches!(
            &t.items()[i],
            TranscriptItem::Elicitation { action: Some(ElicitationAction::Accept), content: Some(c), .. }
                if c == &json!({"env":"prod","confirm":true})
        ));
        assert!(t.waiting_on().is_none());
        assert!(
            t.resolve_elicitation("el-1", ElicitationAction::Decline, None)
                .is_none(),
            "answered once"
        );
    }

    /// A request recorded from CLI 2.1.286 (an MCP server's `elicitation/create`);
    /// the CLI took this exact reply and handed the content to the server.
    #[test]
    fn answers_a_recorded_request_as_the_cli_expects() {
        let line = include_str!("fixtures/elicitation-2.1.286.jsonl");
        let mut t = Transcript::default();
        let a = t.apply_line(line);
        assert!(a.reply.is_none() && a.unknown_kind.is_none());
        let id = t.waiting_on().unwrap().id().to_string();
        let content = json!({"env":"staging","replicas":2,"confirm":true});
        let (_, reply) = t
            .resolve_elicitation(&id, ElicitationAction::Accept, content.as_object())
            .unwrap();
        assert_eq!(
            reply,
            json!({"type":"control_response","response":{"subtype":"success","request_id":id,
                "response":{"action":"accept","content":content}}})
        );
    }

    #[test]
    fn url_elicitations_decline_expire_and_complete() {
        let url = |id: &str| {
            request(
                id,
                json!({"subtype":"elicitation","mcp_server_name":"github","message":"Sign in",
                "mode":"url","url":"https://example.com/auth","elicitation_id":format!("e-{id}")}),
            )
        };
        let mut t = Transcript::default();
        t.apply(&url("a"));
        t.apply(&url("b"));
        let (_, response) = t
            .resolve_elicitation("a", ElicitationAction::Decline, Some(&Map::new()))
            .unwrap();
        assert_eq!(
            response["response"]["response"],
            json!({"action":"decline"})
        );

        let a = t.apply(&json!({"type":"control_cancel_request","request_id":"b"}));
        assert_eq!(a.items, vec![1]);
        assert!(matches!(
            &t.items()[1],
            TranscriptItem::Elicitation { expired: true, .. }
        ));
        assert!(t.waiting_on().is_none());

        let a = t.apply(&json!({"type":"system","subtype":"elicitation_complete",
            "mcp_server_name":"github","elicitation_id":"e-a","uuid":"u","session_id":"s"}));
        assert!(a.unknown_kind.is_none());
        assert_eq!(a.items, vec![0]);
        let TranscriptItem::Elicitation {
            completed,
            url,
            schema,
            ..
        } = &t.items()[0]
        else {
            panic!()
        };
        assert!(*completed);
        assert_eq!(url.as_deref(), Some("https://example.com/auth"));
        assert!(schema.is_none());
        let json = serde_json::to_value(&t.items()[0]).unwrap();
        assert!(json.get("elicitationId").is_none(), "kept server-side");
    }

    #[test]
    fn hook_callbacks_and_dialogs_still_get_an_error_reply() {
        let mut t = Transcript::default();
        for (id, subtype) in [("h", "hook_callback"), ("d", "request_user_dialog")] {
            let a = t.apply(&request(id, json!({ "subtype": subtype })));
            assert_eq!(a.reply.unwrap()["response"]["subtype"], "error");
        }
        assert!(t.items().is_empty());
    }
}
