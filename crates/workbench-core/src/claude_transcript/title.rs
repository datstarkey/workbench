//! Titles in Claude's session JSONL. A rename takes precedence over an AI title,
//! even if the CLI writes another generated title afterwards.

use serde_json::Value;

use super::parse::str_at;

#[derive(Debug, Default)]
pub(crate) struct SavedTitle {
    custom: Option<String>,
    generated: Option<String>,
}

impl SavedTitle {
    pub fn apply(&mut self, entry: &Value) {
        let (slot, key) = match str_at(entry, "type") {
            Some("custom-title") => (&mut self.custom, "customTitle"),
            Some("ai-title") => (&mut self.generated, "aiTitle"),
            _ => return,
        };
        if let Some(title) = str_at(entry, key).map(str::trim).filter(|s| !s.is_empty()) {
            *slot = Some(title.to_string());
        }
    }

    pub fn get(&self) -> Option<&str> {
        self.custom.as_deref().or(self.generated.as_deref())
    }
}
