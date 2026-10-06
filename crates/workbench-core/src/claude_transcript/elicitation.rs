//! MCP elicitation: an MCP server asking the person for input mid-turn (MCP
//! `elicitation/create`). Codex forwards it as `mcpServer/elicitation/request`,
//! answered from the chat with an MCP `ElicitResult`. A terminal `claude` asks
//! in its own dialog: the plugin names it ([`TERMINAL_ELICITATION`]) and its
//! answer ([`TERMINAL_ELICITATION_ANSWERED`]), and the chat shows a read-only item.
//!
//! Form mode carries a flat JSON schema of primitive fields; URL mode asks the
//! person to open a page (OAuth, payment).

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{clip, clip_value, TranscriptItem};

/// The Workbench plugin's line for an elicitation its terminal shows.
pub const TERMINAL_ELICITATION: &str = "workbench_terminal_elicitation";
/// The plugin's line for how the terminal answered one (`id`, `action`).
pub const TERMINAL_ELICITATION_ANSWERED: &str = "workbench_terminal_elicitation_result";

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
            in_terminal: false,
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
    use super::*;

    #[test]
    fn form_answers_carry_only_the_declared_fields() {
        let schema = json!({"type":"object","properties":{
            "env":{"type":"string","enum":["staging","prod"]},
            "replicas":{"type":"integer"},
            "confirm":{"type":"boolean"}},"required":["env"]});
        let (item, schema) = Request {
            id: "el-1".into(),
            server: "deploy",
            message: "Which environment?",
            mode: Some("form"),
            url: None,
            elicitation_id: None,
            schema: Some(&schema),
            title: None,
            description: None,
        }
        .into_item();
        let mut items = vec![item];
        let content = json!({"env":"prod","replicas":2.5,"confirm":true,"extra":"x"});
        let response = Pending::new(0, schema).answer(
            &mut items,
            ElicitationAction::Accept,
            content.as_object(),
        );
        assert_eq!(
            response,
            json!({"action":"accept","content":{"env":"prod","confirm":true}})
        );
        assert!(matches!(
            &items[0],
            TranscriptItem::Elicitation { action: Some(ElicitationAction::Accept), content: Some(c), .. }
                if c == &json!({"env":"prod","confirm":true})
        ));
    }
}
