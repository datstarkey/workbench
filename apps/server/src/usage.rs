//! Claude plan usage (`claude -p /usage`) per account, for the chat views.
//! Each check starts the CLI and asks Anthropic (~2s), and every open chat on
//! every device polls, so results are cached per account and concurrent
//! requests share one run.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use workbench_core::claude_accounts::{self, UsageLimit};

const TTL: Duration = Duration::from_secs(60);

/// The last outcome for one account, failures included, so a timing-out CLI
/// isn't retried by every waiting request in turn.
type Slot = Arc<tokio::sync::Mutex<Option<(Instant, Result<Vec<UsageLimit>, String>)>>>;

#[derive(Clone, Default)]
pub struct UsageCache {
    slots: Arc<Mutex<HashMap<Option<String>, Slot>>>,
}

impl UsageCache {
    /// `account_id` must already be a known account (see `claude_accounts::resolve`),
    /// so request input can't grow the map.
    pub async fn get(&self, account_id: Option<String>) -> Result<Vec<UsageLimit>> {
        let id = account_id.clone();
        self.get_with(account_id, move || claude_accounts::usage(id.as_deref()))
            .await
    }

    async fn get_with<F>(&self, account_id: Option<String>, fetch: F) -> Result<Vec<UsageLimit>>
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
            if at.elapsed() < TTL {
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
            cache.get_with(None, fetch(runs.clone())),
            cache.get_with(None, fetch(runs.clone())),
        );
        assert_eq!(a.unwrap(), limit(42));
        assert_eq!(b.unwrap(), limit(42));
        cache.get_with(None, fetch(runs.clone())).await.unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 1);

        // Another account has its own entry.
        cache
            .get_with(Some("work".into()), fetch(runs.clone()))
            .await
            .unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failures_are_cached_too() {
        let cache = UsageCache::default();
        let err = cache
            .get_with(None, || anyhow::bail!("timed out"))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "timed out");
        let again = cache.get_with(None, || Ok(limit(1))).await;
        assert!(again.is_err());
    }
}
