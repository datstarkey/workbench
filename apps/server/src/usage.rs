//! Claude plan usage (`claude -p /usage`) per account, for the chat views.
//! Each check starts the CLI and asks Anthropic (~2s), and every open chat on
//! every device polls, so results are cached per account and concurrent
//! requests share one run. A `fresh` request (a turn just ended) accepts only
//! a run from the last few seconds, so a burst of them still shares one.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use workbench_core::claude_accounts::{self, UsageLimit};

const TTL: Duration = Duration::from_secs(60);
const FRESH_TTL: Duration = Duration::from_secs(10);

/// The last outcome for one account, failures included, so a timing-out CLI
/// isn't retried by every waiting request in turn.
type Slot = Arc<tokio::sync::Mutex<Option<(Instant, Result<Vec<UsageLimit>, String>)>>>;

#[derive(Clone)]
pub struct UsageCache {
    slots: Arc<Mutex<HashMap<Option<String>, Slot>>>,
    fresh_ttl: Duration,
}

impl Default for UsageCache {
    fn default() -> Self {
        Self::with_fresh_ttl(FRESH_TTL)
    }
}

impl UsageCache {
    /// How old a result a `fresh` request still accepts (tests shorten it).
    pub fn with_fresh_ttl(fresh_ttl: Duration) -> Self {
        Self {
            slots: Arc::default(),
            fresh_ttl,
        }
    }

    /// `account_id` must already be a known account (see `claude_accounts::resolve`),
    /// so request input can't grow the map.
    pub async fn get(&self, account_id: Option<String>, fresh: bool) -> Result<Vec<UsageLimit>> {
        let id = account_id.clone();
        self.get_with(account_id, fresh, move || {
            claude_accounts::usage(id.as_deref())
        })
        .await
    }

    async fn get_with<F>(
        &self,
        account_id: Option<String>,
        fresh: bool,
        fetch: F,
    ) -> Result<Vec<UsageLimit>>
    where
        F: FnOnce() -> Result<Vec<UsageLimit>> + Send + 'static,
    {
        let slot = self
            .slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(account_id)
            .or_default()
            .clone();
        // Held across the run: concurrent requests wait for it rather than start their own.
        let mut cached = slot.lock().await;
        if let Some((at, result)) = cached.as_ref() {
            if at.elapsed() < if fresh { self.fresh_ttl } else { TTL } {
                return result.clone().map_err(anyhow::Error::msg);
            }
        }
        let result = tokio::task::spawn_blocking(fetch)
            .await?
            .map_err(|e| format!("{e:#}"));
        *cached = Some((Instant::now(), result.clone()));
        result.map_err(anyhow::Error::msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn limit(percent: u8) -> Vec<UsageLimit> {
        vec![UsageLimit {
            label: "session".into(),
            percent,
            resets: None,
        }]
    }

    #[tokio::test]
    async fn concurrent_requests_share_one_run_and_later_ones_hit_the_cache() {
        let cache = UsageCache::default();
        let runs = Arc::new(AtomicUsize::new(0));
        let fetch = |runs: Arc<AtomicUsize>| {
            move || {
                runs.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(100));
                Ok(limit(42))
            }
        };
        let (a, b) = tokio::join!(
            cache.get_with(None, false, fetch(runs.clone())),
            cache.get_with(None, false, fetch(runs.clone())),
        );
        assert_eq!(a.unwrap(), limit(42));
        assert_eq!(b.unwrap(), limit(42));
        cache
            .get_with(None, false, fetch(runs.clone()))
            .await
            .unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 1);

        // Another account has its own entry.
        cache
            .get_with(Some("work".into()), false, fetch(runs.clone()))
            .await
            .unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn fresh_requests_rerun_only_past_the_floor() {
        let cache = UsageCache::with_fresh_ttl(Duration::from_millis(50));
        cache.get_with(None, false, || Ok(limit(1))).await.unwrap();
        let shared = cache.get_with(None, true, || Ok(limit(2))).await.unwrap();
        assert_eq!(
            shared,
            limit(1),
            "within the floor a fresh request shares the run"
        );
        tokio::time::sleep(Duration::from_millis(60)).await;
        let rerun = cache.get_with(None, true, || Ok(limit(3))).await.unwrap();
        assert_eq!(rerun, limit(3));
        let cached = cache.get_with(None, false, || Ok(limit(4))).await.unwrap();
        assert_eq!(cached, limit(3), "a normal request takes the fresh result");
    }

    #[tokio::test]
    async fn failures_are_cached_too() {
        let cache = UsageCache::default();
        let err = cache
            .get_with(None, false, || anyhow::bail!("timed out"))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "timed out");
        let again = cache.get_with(None, false, || Ok(limit(1))).await;
        assert!(again.is_err());
    }
}
