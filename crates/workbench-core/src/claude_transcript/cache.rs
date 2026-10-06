//! Prompt cache timing: when the cache the latest API call used expires, and
//! the keep-alive turn that refreshes it without showing in the chat.

use std::path::Path;

use serde_json::Value;

use super::{str_at, Transcript, TranscriptItem};

/// What a keep-alive turn sends. Any call that reads the cached prefix starts
/// its lifetime over; the chat shows the turn as one notice.
pub const KEEPALIVE_PROMPT: &str = "Workbench cache keep-alive. Reply with only \"ok\".";

/// Assumed until a call writes the cache and so names its lifetime.
const DEFAULT_TTL_SECS: u64 = 300;

impl Transcript {
    pub(super) fn note_cache(&mut self, obj: &Value, usage: &Value) {
        let n = |k| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
        if n("cache_read_input_tokens") + n("cache_creation_input_tokens") == 0 {
            return;
        }
        let ttl = written_ttl_secs(usage)
            .or(self.meta.cache_ttl_secs)
            .unwrap_or(DEFAULT_TTL_SECS);
        self.meta.cache_ttl_secs = Some(ttl);
        self.meta.cache_expires_at = Some(line_time_ms(obj) + ttl * 1000);
    }

    /// Set the cache's lifetime learned elsewhere (a terminal's plugin reports
    /// usage without the split, so its session reads the transcript file),
    /// moving the expiry to match. False when nothing changed.
    pub fn learn_cache_ttl(&mut self, ttl: u64) -> bool {
        if self.meta.cache_ttl_secs == Some(ttl) {
            return false;
        }
        let old = self
            .meta
            .cache_ttl_secs
            .replace(ttl)
            .unwrap_or(DEFAULT_TTL_SECS);
        if let Some(at) = &mut self.meta.cache_expires_at {
            *at = (*at + ttl * 1000).saturating_sub(old * 1000);
        }
        true
    }

    /// A keep-alive prompt's echo: true if `text` is one, after recording it.
    pub(super) fn start_keepalive(
        &mut self,
        id: String,
        text: &str,
        changed: &mut Vec<usize>,
    ) -> bool {
        if text != KEEPALIVE_PROMPT {
            return false;
        }
        self.keepalive = true;
        self.meta.busy = true;
        let text = "Cache kept warm".to_string();
        self.upsert(TranscriptItem::Notice { id, text }, changed);
        true
    }
}

/// The lifetime a call wrote its cache with; `None` when it only read, or
/// its usage has no `cache_creation` split (a terminal's plugin) to say.
fn written_ttl_secs(usage: &Value) -> Option<u64> {
    let split = usage.get("cache_creation")?;
    let n = |k| split.get(k).and_then(Value::as_u64).unwrap_or(0);
    if n("ephemeral_1h_input_tokens") > 0 {
        Some(3600)
    } else if n("ephemeral_5m_input_tokens") > 0 {
        Some(300)
    } else {
        None
    }
}

/// How much of a session JSONL's end is searched for the newest cache write.
const TAIL_BYTES: u64 = 512 * 1024;

/// The lifetime the newest call near the end of a session JSONL wrote its
/// cache with. Only the tail is read: transcripts run to many megabytes.
pub fn written_cache_ttl(path: &Path) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let start = file.metadata().ok()?.len().saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // A cut first line is never whole JSON, so parsing skips it.
    text.lines().rev().find_map(|line| {
        let obj: Value = serde_json::from_str(line).ok()?;
        if str_at(&obj, "type") != Some("assistant") {
            return None;
        }
        written_ttl_secs(obj.pointer("/message/usage")?)
    })
}

/// When a line was written: its `timestamp` (JSONL history), else now (a live event).
fn line_time_ms(obj: &Value) -> u64 {
    str_at(obj, "timestamp")
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map_or_else(
            || chrono::Utc::now().timestamp_millis(),
            |t| t.timestamp_millis(),
        )
        .max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_transcript::ChatView;
    use serde_json::json;

    fn call(timestamp: &str, usage: Value) -> Value {
        json!({"type":"assistant","uuid":"a","timestamp":timestamp,"message":{"id":"m",
            "content":[{"type":"text","text":"ok"}],"usage":usage}})
    }

    const T0: u64 = 1_790_888_830_000; // 2026-10-01T21:07:10Z

    #[test]
    fn expiry_follows_the_ttl_the_cache_was_written_with() {
        let mut t = Transcript::default();
        t.apply(&call(
            "2026-10-01T21:07:10Z",
            json!({"cache_creation_input_tokens":500,
                "cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":500}}),
        ));
        assert_eq!(t.meta().cache_ttl_secs, Some(3600));
        assert_eq!(t.meta().cache_expires_at, Some(T0 + 3_600_000));

        // A read-only call keeps the lifetime and restarts it.
        t.apply(&call(
            "2026-10-01T21:17:10Z",
            json!({"cache_read_input_tokens":500}),
        ));
        assert_eq!(t.meta().cache_expires_at, Some(T0 + 600_000 + 3_600_000));
    }

    #[test]
    fn flat_usage_keeps_the_ttl_until_one_is_learned() {
        // A terminal's plugin: counts only, no `cache_creation` split.
        let flat = json!({"cache_creation_input_tokens":500});
        let mut t = Transcript::default();
        t.apply(&call("2026-10-01T21:07:10Z", flat.clone()));
        assert_eq!(t.meta().cache_expires_at, Some(T0 + 300_000));

        assert!(t.learn_cache_ttl(3600));
        assert_eq!(t.meta().cache_ttl_secs, Some(3600));
        assert_eq!(t.meta().cache_expires_at, Some(T0 + 3_600_000));
        assert!(!t.learn_cache_ttl(3600));

        // Later flat writes keep the learned lifetime.
        t.apply(&call("2026-10-01T21:17:10Z", flat));
        assert_eq!(t.meta().cache_expires_at, Some(T0 + 600_000 + 3_600_000));
    }

    #[test]
    fn the_newest_split_in_a_jsonl_names_the_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let lines = [
            call(
                "2026-10-01T21:07:10Z",
                json!({"cache_creation_input_tokens":500,
                "cache_creation":{"ephemeral_5m_input_tokens":500,"ephemeral_1h_input_tokens":0}}),
            ),
            call(
                "2026-10-01T21:08:10Z",
                json!({"cache_creation_input_tokens":500,
                "cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":500}}),
            ),
            call(
                "2026-10-01T21:09:10Z",
                json!({"cache_read_input_tokens":500,
                "cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0}}),
            ),
        ];
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        std::fs::write(&path, text.join("\n")).unwrap();
        assert_eq!(written_cache_ttl(&path), Some(3600));
        assert_eq!(written_cache_ttl(&dir.path().join("missing.jsonl")), None);
    }

    #[test]
    fn unknown_ttl_assumes_five_minutes_and_a_compact_clears_it() {
        let mut t = Transcript::default();
        t.apply(&call(
            "2026-10-01T21:07:10Z",
            json!({"cache_read_input_tokens":500}),
        ));
        assert_eq!(t.meta().cache_expires_at, Some(T0 + 300_000));
        t.apply(&json!({"type":"system","subtype":"compact_boundary","uuid":"c"}));
        assert_eq!(t.meta().cache_expires_at, None);
        assert_eq!(t.meta().context_tokens, None);
    }

    #[test]
    fn a_keepalive_turn_shows_as_one_notice() {
        let mut t = Transcript::default();
        t.apply(&json!({"type":"user","uuid":"k","message":{"content":KEEPALIVE_PROMPT}}));
        assert!(t.meta().busy);
        t.apply(
            &json!({"type":"stream_event","event":{"type":"message_start","message":{"id":"m"}}}),
        );
        t.apply(
            &json!({"type":"stream_event","event":{"type":"content_block_start","index":0,
            "content_block":{"type":"text","text":""}}}),
        );
        t.apply(&call(
            "2026-10-01T21:07:10Z",
            json!({"cache_read_input_tokens":500}),
        ));
        t.apply(&json!({"type":"result","subtype":"success"}));

        assert_eq!(t.items().len(), 1);
        assert!(
            matches!(&t.items()[0], TranscriptItem::Notice { text, .. } if text == "Cache kept warm")
        );
        assert!(t.meta().cache_expires_at.is_some());
        assert!(!t.meta().busy);

        // A prompt queued into a keep-alive turn shows its reply.
        let mut queued = Transcript::default();
        queued.apply(&json!({"type":"user","uuid":"k","message":{"content":KEEPALIVE_PROMPT}}));
        queued.apply(&json!({"type":"attachment","uuid":"q","attachment":{
            "type":"queued_command","commandMode":"prompt","prompt":"real question"}}));
        queued.apply(&call("2026-10-01T21:07:10Z", json!({})));
        assert_eq!(queued.items().len(), 3);

        // The next real turn shows again.
        t.apply(&json!({"type":"user","uuid":"u","message":{"content":"next"}}));
        t.apply(&call("2026-10-01T21:08:10Z", json!({})));
        assert_eq!(t.items().len(), 3);
    }
}
