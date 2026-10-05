//! Per-chat prompt cache upkeep (Claude only): keep the cache warm with hidden
//! keep-alive turns until a set time, and/or compact the conversation just
//! before the cache expires, so the next turn doesn't write it all again.
//! Policies are saved by session id, so a chat keeps its own across restarts.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use workbench_core::claude_transcript::TranscriptMeta;

use super::lock;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePolicy {
    /// Unix ms; keep-alive turns run until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_warm_until: Option<u64>,
    #[serde(default)]
    pub compact_on_expiry: bool,
}

/// How often sessions are checked.
pub(super) const TICK: Duration = Duration::from_secs(15);
/// How long before expiry upkeep acts: a check can land a tick late, and the
/// turn still has to reach the API.
const LEAD_MS: u64 = 120_000;
/// Below this a compact saves too little to be worth a turn.
const MIN_COMPACT_TOKENS: u64 = 30_000;

#[derive(Debug, PartialEq)]
pub(super) enum Upkeep {
    KeepAlive,
    Compact,
}

/// What `policy` calls for at `now`, for a session that is `idle` (no turn,
/// nothing waiting on an answer).
pub(super) fn due(
    policy: &CachePolicy,
    meta: &TranscriptMeta,
    idle: bool,
    now: u64,
) -> Option<Upkeep> {
    let expires = meta.cache_expires_at?;
    if !idle || now >= expires || expires - now > LEAD_MS {
        return None;
    }
    if policy.keep_warm_until.is_some_and(|until| now < until) {
        return Some(Upkeep::KeepAlive);
    }
    (policy.compact_on_expiry && meta.context_tokens.unwrap_or(0) >= MIN_COMPACT_TOKENS)
        .then_some(Upkeep::Compact)
}

/// `~/.workbench/chat-cache.json`: session id → policy. Default policies
/// aren't kept.
pub(super) struct PolicyStore {
    path: PathBuf,
    map: Mutex<Option<HashMap<String, CachePolicy>>>,
}

impl Default for PolicyStore {
    fn default() -> Self {
        Self {
            path: workbench_core::paths::workbench_config_dir().join("chat-cache.json"),
            map: Mutex::new(None),
        }
    }
}

impl PolicyStore {
    pub fn get(&self, session_id: &str) -> CachePolicy {
        let mut map = lock(&self.map);
        let map =
            map.get_or_insert_with(|| workbench_core::paths::load_json(&self.path, HashMap::new()));
        map.get(session_id).cloned().unwrap_or_default()
    }

    pub fn set(&self, session_id: &str, policy: &CachePolicy) {
        let mut map = lock(&self.map);
        let map =
            map.get_or_insert_with(|| workbench_core::paths::load_json(&self.path, HashMap::new()));
        if *policy == CachePolicy::default() {
            map.remove(session_id);
        } else {
            map.insert(session_id.to_string(), policy.clone());
        }
        if let Err(e) = workbench_core::paths::save_json(&self.path, map) {
            tracing::warn!("could not save chat cache policies: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000_000;

    fn meta(expires_in: i64, tokens: u64) -> TranscriptMeta {
        TranscriptMeta {
            cache_expires_at: Some((NOW as i64 + expires_in) as u64),
            context_tokens: Some(tokens),
            ..TranscriptMeta::default()
        }
    }

    #[test]
    fn acts_only_just_before_expiry_while_idle() {
        let warm = CachePolicy {
            keep_warm_until: Some(NOW + 1),
            compact_on_expiry: false,
        };
        assert_eq!(
            due(&warm, &meta(60_000, 0), true, NOW),
            Some(Upkeep::KeepAlive)
        );
        assert_eq!(due(&warm, &meta(600_000, 0), true, NOW), None);
        assert_eq!(due(&warm, &meta(60_000, 0), false, NOW), None);
        // Already cold: a keep-alive would only write it all again.
        assert_eq!(due(&warm, &meta(-1, 0), true, NOW), None);
    }

    #[test]
    fn compacts_once_keep_warm_ends_and_only_a_large_context() {
        let both = CachePolicy {
            keep_warm_until: Some(NOW),
            compact_on_expiry: true,
        };
        assert_eq!(
            due(&both, &meta(60_000, 80_000), true, NOW),
            Some(Upkeep::Compact)
        );
        assert_eq!(due(&both, &meta(60_000, 5_000), true, NOW), None);
        assert_eq!(
            due(&CachePolicy::default(), &meta(60_000, 80_000), true, NOW),
            None
        );
    }
}
