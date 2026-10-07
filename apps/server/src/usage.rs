//! Claude plan usage and model lists per account, for the chat views.
//!
//! Plan usage comes from the Workbench plugin while a session of that account
//! runs: every API response reports the 5-hour and weekly windows, and the
//! plugin forwards them ([`UsageCache::note`]). Without such a reading the
//! server asks `claude -p /usage`, which starts the CLI and asks Anthropic
//! (~2s); every open chat on every device polls, so results are cached per
//! account and concurrent requests share one run. A `fresh` request (a turn
//! just ended) accepts only a run from the last few seconds, so a burst of
//! them still shares one.

use std::collections::HashMap;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use workbench_core::claude_accounts::{self, RateWindow, UsageLimit};
use workbench_core::claude_transcript::ModelOption;

const TTL: Duration = Duration::from_secs(60);
const FRESH_TTL: Duration = Duration::from_secs(10);
/// The model list only changes with a CLI update or a new login. Kept on disk,
/// and served stale while a newer one is fetched, so a chat never waits on it
/// once an account and cwd have been seen.
const MODELS_TTL: Duration = Duration::from_secs(60 * 60);
/// A failed run is kept this long at most, so a slow start isn't replayed for an hour.
const FAILURE_TTL: Duration = Duration::from_secs(30);
/// How often `/usage` is still run beside a live reading, for the per-model
/// weekly limits only it prints.
const PER_MODEL_TTL: Duration = Duration::from_secs(15 * 60);
/// How long a reading without reset times stands: the session window's length.
const READING_TTL: Duration = Duration::from_secs(5 * 60 * 60);

type Entry<T> = (Instant, Result<T, String>);

/// One key's last outcome, failures included (so a timing-out CLI isn't
/// retried by every waiting request in turn), and the lock a run holds.
struct Slot<T> {
    value: Mutex<Option<Entry<T>>>,
    run: tokio::sync::Mutex<()>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            value: Mutex::new(None),
            run: tokio::sync::Mutex::new(()),
        }
    }
}

impl<T: Clone> Slot<T> {
    fn held(&self) -> Option<Entry<T>> {
        self.value.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The outcome, if younger than `max` (a failure's at most [`FAILURE_TTL`]).
    fn within(&self, max: Duration) -> Option<Result<T, String>> {
        let (at, result) = self.held()?;
        let max = if result.is_err() {
            max.min(FAILURE_TTL)
        } else {
            max
        };
        (at.elapsed() < max).then_some(result)
    }
}

/// A good result as kept on disk.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved<K, T> {
    key: K,
    /// Unix seconds.
    saved_at: u64,
    value: T,
}

/// One CLI run's result per key (an account), shared by concurrent requests.
pub struct AccountCache<K, T> {
    slots: Arc<Mutex<HashMap<K, Arc<Slot<T>>>>>,
    ttl: Duration,
    fresh_ttl: Duration,
    /// Where good results outlive a restart, if anywhere.
    file: Option<PathBuf>,
}

impl<K, T> Clone for AccountCache<K, T> {
    fn clone(&self) -> Self {
        Self {
            slots: self.slots.clone(),
            ttl: self.ttl,
            fresh_ttl: self.fresh_ttl,
            file: self.file.clone(),
        }
    }
}

/// The models an account can pick in a cwd (`claude_accounts::models`).
pub type ModelsCache = AccountCache<(Option<String>, PathBuf), Vec<ModelOption>>;

impl Default for ModelsCache {
    fn default() -> Self {
        Self::new(MODELS_TTL, MODELS_TTL)
            .persisted(workbench_core::paths::workbench_config_dir().join("models-cache.json"))
    }
}

impl ModelsCache {
    /// `account_id` must already be a known account (see `claude_accounts::resolve`)
    /// and `cwd` a session's, so request input can't grow the map.
    pub async fn get(&self, account_id: Option<String>, cwd: PathBuf) -> Result<Vec<ModelOption>> {
        let key = (account_id.clone(), cwd.clone());
        self.get_stale_with(key, move || {
            claude_accounts::models(account_id.as_deref(), &cwd)
        })
        .await
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl<K, T> AccountCache<K, T>
where
    K: Eq + Hash + Clone + Send + Sync + Serialize + DeserializeOwned + 'static,
    T: Clone + Send + Sync + Serialize + DeserializeOwned + 'static,
{
    fn new(ttl: Duration, fresh_ttl: Duration) -> Self {
        Self {
            slots: Arc::default(),
            ttl,
            fresh_ttl,
            file: None,
        }
    }

    /// Keep good results in `file`, starting from what it holds.
    fn persisted(mut self, file: PathBuf) -> Self {
        let saved: Vec<Saved<K, T>> = workbench_core::paths::load_json(&file, Vec::new());
        let now = unix_now();
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        for entry in saved {
            let age = Duration::from_secs(now.saturating_sub(entry.saved_at));
            // Older than this process's clock reaches: due a refresh.
            let at = Instant::now()
                .checked_sub(age)
                .or_else(|| Instant::now().checked_sub(self.ttl))
                .unwrap_or_else(Instant::now);
            let slot = Slot::default();
            *slot.value.lock().unwrap_or_else(|e| e.into_inner()) = Some((at, Ok(entry.value)));
            slots.insert(entry.key, Arc::new(slot));
        }
        drop(slots);
        self.file = Some(file);
        self
    }

    fn save(&self) {
        let Some(file) = &self.file else {
            return;
        };
        // Held while writing, so two saves can't interleave on the temp file.
        let slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let now = unix_now();
        let saved: Vec<Saved<&K, T>> = slots
            .iter()
            .filter_map(|(key, slot)| match slot.held()? {
                (at, Ok(value)) => Some(Saved {
                    key,
                    saved_at: now.saturating_sub(at.elapsed().as_secs()),
                    value,
                }),
                _ => None,
            })
            .collect();
        if let Err(e) = workbench_core::paths::save_json(file, &saved) {
            tracing::warn!("could not save {}: {e:#}", file.display());
        }
    }

    fn slot(&self, key: K) -> Arc<Slot<T>> {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key)
            .or_default()
            .clone()
    }

    /// The result, run now unless one younger than the TTL is cached.
    async fn get_with<F>(&self, key: K, fresh: bool, fetch: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let slot = self.slot(key);
        let max = if fresh { self.fresh_ttl } else { self.ttl };
        if let Some(result) = slot.within(max) {
            return result.map_err(anyhow::Error::msg);
        }
        // Concurrent requests wait for one run rather than start their own.
        let _run = slot.run.lock().await;
        if let Some(result) = slot.within(max) {
            return result.map_err(anyhow::Error::msg);
        }
        let result = tokio::task::spawn_blocking(fetch)
            .await?
            .map_err(|e| format!("{e:#}"));
        *slot.value.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((Instant::now(), result.clone()));
        if result.is_ok() {
            self.save();
        }
        result.map_err(anyhow::Error::msg)
    }

    /// A good result at once, however old, refreshed behind once past the
    /// TTL. Only a key with no good result waits for a run.
    async fn get_stale_with<F>(&self, key: K, fetch: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        match self.slot(key.clone()).held() {
            Some((at, Ok(value))) => {
                if at.elapsed() >= self.ttl {
                    self.refresh_behind(key, fetch);
                }
                Ok(value)
            }
            _ => self.get_with(key, false, fetch).await,
        }
    }

    /// The last good result, if any, never waiting; a run starts behind when
    /// there is none or it is older than `max`.
    fn peek_with<F>(&self, key: K, max: Duration, fetch: F) -> Option<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let held = self.slot(key.clone()).held();
        let good = match &held {
            Some((at, Ok(value))) => Some((*at, value.clone())),
            _ => None,
        };
        if good.as_ref().is_none_or(|(at, _)| at.elapsed() >= max) {
            self.refresh_behind(key, fetch);
        }
        good.map(|(_, value)| value)
    }

    fn refresh_behind<F>(&self, key: K, fetch: F)
    where
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let cache = self.clone();
        tokio::spawn(async move {
            if let Err(e) = cache.get_with(key, false, fetch).await {
                tracing::warn!("background refresh failed: {e:#}");
            }
        });
    }
}

/// A live session's latest plan usage for one account.
#[derive(Clone)]
struct Reading {
    at: Instant,
    limits: Vec<UsageLimit>,
}

impl Reading {
    /// Until a window it reports resets, after which its figure is stale.
    fn current(&self) -> bool {
        let now = unix_now();
        self.limits.iter().all(|l| match l.resets_at {
            Some(reset) => reset > now,
            None => self.at.elapsed() < READING_TTL,
        })
    }
}

/// Plan usage per account: live sessions' readings, else `claude -p /usage`.
#[derive(Clone)]
pub struct UsageCache {
    probe: AccountCache<Option<String>, Vec<UsageLimit>>,
    readings: Arc<Mutex<HashMap<Option<String>, Reading>>>,
}

impl Default for UsageCache {
    fn default() -> Self {
        Self::with_fresh_ttl(FRESH_TTL)
    }
}

/// A weekly limit for one model, which only `/usage` prints.
fn is_per_model(limit: &UsageLimit) -> bool {
    limit.label.starts_with("week (") && limit.label != "week (all models)"
}

impl UsageCache {
    /// How old a result a `fresh` request still accepts (tests shorten it).
    pub fn with_fresh_ttl(fresh_ttl: Duration) -> Self {
        Self {
            probe: AccountCache::new(TTL, fresh_ttl),
            readings: Arc::default(),
        }
    }

    /// The windows a session of `account_id` just reported (the plugin's
    /// `rate_limit_event`). An empty list (off a subscription) is no reading.
    pub fn note(&self, account_id: Option<String>, windows: &[RateWindow]) {
        if windows.is_empty() {
            return;
        }
        let reading = Reading {
            at: Instant::now(),
            limits: claude_accounts::limits_from_windows(windows),
        };
        self.readings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(account_id, reading);
    }

    fn reading(&self, account_id: &Option<String>) -> Option<Vec<UsageLimit>> {
        let readings = self.readings.lock().unwrap_or_else(|e| e.into_inner());
        let reading = readings.get(account_id).filter(|r| r.current())?;
        Some(reading.limits.clone())
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
        probe: F,
    ) -> Result<Vec<UsageLimit>>
    where
        F: FnOnce() -> Result<Vec<UsageLimit>> + Send + 'static,
    {
        let Some(mut limits) = self.reading(&account_id) else {
            return self.probe.get_with(account_id, fresh, probe).await;
        };
        let probed = self
            .probe
            .peek_with(account_id, PER_MODEL_TTL, probe)
            .unwrap_or_default();
        let per_model: Vec<_> = probed
            .into_iter()
            .filter(|p| {
                is_per_model(p)
                    && !limits
                        .iter()
                        .any(|l| l.label.eq_ignore_ascii_case(&p.label))
            })
            .collect();
        limits.extend(per_model);
        Ok(limits)
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

    fn probe() -> AccountCache<Option<String>, Vec<UsageLimit>> {
        AccountCache::new(TTL, FRESH_TTL)
    }

    #[tokio::test]
    async fn concurrent_requests_share_one_run_and_later_ones_hit_the_cache() {
        let cache = probe();
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
        let cache = probe();
        let cache = AccountCache {
            fresh_ttl: Duration::from_millis(50),
            ..cache
        };
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
        let cache = probe();
        let err = cache
            .get_with(None, false, || anyhow::bail!("timed out"))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "timed out");
        let again = cache.get_with(None, false, || Ok(limit(1))).await;
        assert!(again.is_err());
    }

    #[tokio::test]
    async fn an_expired_result_is_served_stale_and_refreshed_behind() {
        let cache: AccountCache<Option<String>, _> =
            AccountCache::new(Duration::from_millis(50), Duration::from_millis(50));
        let first = cache.get_stale_with(None, || Ok(limit(1))).await.unwrap();
        assert_eq!(first, limit(1), "a missing entry waits for its run");
        tokio::time::sleep(Duration::from_millis(60)).await;
        let slow = || {
            std::thread::sleep(Duration::from_millis(100));
            Ok(limit(2))
        };
        let started = Instant::now();
        let stale = cache.get_stale_with(None, slow).await.unwrap();
        assert_eq!(stale, limit(1), "past the TTL the old result comes at once");
        assert!(started.elapsed() < Duration::from_millis(50));
        tokio::time::sleep(Duration::from_millis(200)).await;
        let refreshed = cache
            .get_stale_with(None, || -> Result<_> { unreachable!("fresh again") })
            .await
            .unwrap();
        assert_eq!(refreshed, limit(2));
    }

    #[tokio::test]
    async fn good_results_outlive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cache.json");
        let key = (Some("work".to_string()), PathBuf::from("/project"));
        let cache = AccountCache::new(TTL, TTL).persisted(file.clone());
        cache
            .get_stale_with(key.clone(), || Ok(limit(5)))
            .await
            .unwrap();
        cache
            .get_stale_with((None, PathBuf::from("/other")), || anyhow::bail!("no"))
            .await
            .unwrap_err();

        let restarted: AccountCache<_, Vec<UsageLimit>> =
            AccountCache::new(TTL, TTL).persisted(file);
        let kept = restarted
            .get_stale_with(key, || -> Result<_> { unreachable!("loaded from disk") })
            .await
            .unwrap();
        assert_eq!(kept, limit(5));
        let failed = restarted
            .get_stale_with((None, PathBuf::from("/other")), || Ok(limit(6)))
            .await
            .unwrap();
        assert_eq!(failed, limit(6), "failures aren't kept");
    }

    fn windows(five_hour: f64, resets_at: u64) -> Vec<RateWindow> {
        serde_json::from_value(serde_json::json!([
            {"kind": "five_hour", "percentUsed": five_hour, "resetsAt": resets_at},
            {"kind": "seven_day", "percentUsed": 40, "resetsAt": resets_at}
        ]))
        .unwrap()
    }

    fn probed() -> Result<Vec<UsageLimit>> {
        let week = |label: &str, percent| UsageLimit {
            label: label.into(),
            percent,
            resets: Some("Oct 2 at 9am (Europe/London)".into()),
            resets_at: None,
        };
        Ok(vec![
            week("session", 1),
            week("week (all models)", 2),
            week("week (Fable)", 3),
        ])
    }

    #[tokio::test]
    async fn a_live_reading_answers_per_account_with_the_probes_per_model_limits() {
        let usage = UsageCache::default();
        let later = unix_now() + 3600;
        usage.note(Some("work".into()), &windows(23.5, later));

        let runs = Arc::new(AtomicUsize::new(0));
        let counted = |runs: Arc<AtomicUsize>| {
            move || {
                runs.fetch_add(1, Ordering::SeqCst);
                probed()
            }
        };
        let first = usage
            .get_with(Some("work".into()), false, counted(runs.clone()))
            .await
            .unwrap();
        let labels: Vec<_> = first
            .iter()
            .map(|l| (l.label.as_str(), l.percent))
            .collect();
        assert_eq!(labels, [("session", 24), ("week (all models)", 40)]);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let second = usage
            .get_with(Some("work".into()), true, counted(runs.clone()))
            .await
            .unwrap();
        assert_eq!(
            second.len(),
            3,
            "the probe's per-model limit joins once run"
        );
        assert_eq!(second[2].label, "week (Fable)");
        assert_eq!(runs.load(Ordering::SeqCst), 1, "run once, behind");

        // The default login's reading reports a window that has reset since:
        // stale, so the probe answers, and is waited for.
        usage.note(None, &windows(90.0, unix_now() - 1));
        let default = usage.get_with(None, false, probed).await.unwrap();
        assert_eq!(default[0].percent, 1);
    }
}
