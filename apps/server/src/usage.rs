//! Claude plan usage (`claude -p /usage`) and model lists per account, for
//! the chat views.
//! Each check starts the CLI and asks Anthropic (~2s), and every open chat on
//! every device polls, so results are cached per account and concurrent
//! requests share one run. A `fresh` request (a turn just ended) accepts only
//! a run from the last few seconds, so a burst of them still shares one.

use std::collections::HashMap;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use workbench_core::claude_accounts::{self, UsageLimit};
use workbench_core::claude_transcript::ModelOption;

const TTL: Duration = Duration::from_secs(60);
const FRESH_TTL: Duration = Duration::from_secs(10);
/// The model list only changes with a CLI update or a new login. In memory
/// only, so a restart (the usual way an update lands) probes again.
const MODELS_TTL: Duration = Duration::from_secs(60 * 60);
/// A failed run is kept this long at most, so a slow start isn't replayed for an hour.
const FAILURE_TTL: Duration = Duration::from_secs(30);

/// The last outcome for one account, failures included, so a timing-out CLI
/// isn't retried by every waiting request in turn.
type Slot<T> = Arc<tokio::sync::Mutex<Option<(Instant, Result<T, String>)>>>;

/// One CLI run's result per key (an account), shared by concurrent requests.
pub struct AccountCache<K, T> {
    slots: Arc<Mutex<HashMap<K, Slot<T>>>>,
    ttl: Duration,
    fresh_ttl: Duration,
}

impl<K, T> Clone for AccountCache<K, T> {
    fn clone(&self) -> Self {
        Self {
            slots: self.slots.clone(),
            ttl: self.ttl,
            fresh_ttl: self.fresh_ttl,
        }
    }
}

/// Plan usage (`claude -p /usage`).
pub type UsageCache = AccountCache<Option<String>, Vec<UsageLimit>>;
/// The models an account can pick in a cwd (`claude_accounts::models`).
pub type ModelsCache = AccountCache<(Option<String>, PathBuf), Vec<ModelOption>>;

impl Default for UsageCache {
    fn default() -> Self {
        Self::with_fresh_ttl(FRESH_TTL)
    }
}

impl Default for ModelsCache {
    fn default() -> Self {
        Self::new(MODELS_TTL, MODELS_TTL)
    }
}

impl UsageCache {
    /// How old a result a `fresh` request still accepts (tests shorten it).
    pub fn with_fresh_ttl(fresh_ttl: Duration) -> Self {
        Self::new(TTL, fresh_ttl)
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
}

impl ModelsCache {
    /// `account_id` must already be a known account and `cwd` a session's,
    /// as for [`UsageCache::get`].
    pub async fn get(&self, account_id: Option<String>, cwd: PathBuf) -> Result<Vec<ModelOption>> {
        let key = (account_id.clone(), cwd.clone());
        self.get_with(key, false, move || {
            claude_accounts::models(account_id.as_deref(), &cwd)
        })
        .await
    }
}

impl<K: Eq + Hash, T: Clone + Send + 'static> AccountCache<K, T> {
    fn new(ttl: Duration, fresh_ttl: Duration) -> Self {
        Self {
            slots: Arc::default(),
            ttl,
            fresh_ttl,
        }
    }

    async fn get_with<F>(&self, key: K, fresh: bool, fetch: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let slot = self
            .slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key)
            .or_default()
            .clone();
        // Held across the run: concurrent requests wait for it rather than start their own.
        let mut cached = slot.lock().await;
        if let Some((at, result)) = cached.as_ref() {
            let max = if fresh { self.fresh_ttl } else { self.ttl };
            let max = if result.is_err() {
                max.min(FAILURE_TTL)
            } else {
                max
            };
            if at.elapsed() < max {
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
            resets_at: None,
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
