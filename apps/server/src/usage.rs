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
use std::sync::atomic::{AtomicU64, Ordering};
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
/// A list saved longer ago than this is not served at all.
const MODELS_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// A failed run is kept this long at most, so a slow start isn't replayed for an hour.
const FAILURE_TTL: Duration = Duration::from_secs(30);
/// How often `/usage` is still run beside a live reading, for the per-model
/// weekly limits only it prints.
const PER_MODEL_TTL: Duration = Duration::from_secs(15 * 60);

/// One key's last good result and last failure, and the lock a run holds. A
/// failure is kept (so a timing-out CLI isn't retried by every waiting
/// request in turn) beside the good result, which it never replaces.
struct Slot<T> {
    good: Mutex<Option<(Instant, T)>>,
    failed: Mutex<Option<(Instant, String)>>,
    run: tokio::sync::Mutex<()>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            good: Mutex::new(None),
            failed: Mutex::new(None),
            run: tokio::sync::Mutex::new(()),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl<T: Clone> Slot<T> {
    fn good(&self) -> Option<(Instant, T)> {
        lock(&self.good).clone()
    }

    /// A good result younger than `max`; else, within [`FAILURE_TTL`] of a
    /// failure, the last good result however old, or that failure.
    fn cached(&self, max: Duration) -> Option<Result<T, String>> {
        let good = self.good();
        if let Some((at, value)) = &good {
            if at.elapsed() < max {
                return Some(Ok(value.clone()));
            }
        }
        let (at, err) = lock(&self.failed).clone()?;
        (at.elapsed() < max.min(FAILURE_TTL)).then(|| good.map(|(_, v)| v).ok_or(err))
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

/// Where a cache's good results outlive a restart.
struct Store {
    file: PathBuf,
    /// Saves are numbered as taken; an older one never overwrites a newer.
    taken: AtomicU64,
    written: Mutex<u64>,
}

/// Called with a result a background refresh fetched.
pub type OnRefresh<T> = Box<dyn FnOnce(T) + Send>;

/// One CLI run's result per key (an account), shared by concurrent requests.
pub struct AccountCache<K, T> {
    slots: Arc<Mutex<HashMap<K, Arc<Slot<T>>>>>,
    ttl: Duration,
    fresh_ttl: Duration,
    store: Option<Arc<Store>>,
    /// Which keys are still worth keeping (a cwd that no longer exists isn't).
    keep: fn(&K) -> bool,
}

impl<K, T> Clone for AccountCache<K, T> {
    fn clone(&self) -> Self {
        Self {
            slots: self.slots.clone(),
            ttl: self.ttl,
            fresh_ttl: self.fresh_ttl,
            store: self.store.clone(),
            keep: self.keep,
        }
    }
}

/// The models an account can pick in a cwd (`claude_accounts::models`).
pub type ModelsCache = AccountCache<(Option<String>, PathBuf), Vec<ModelOption>>;

impl Default for ModelsCache {
    fn default() -> Self {
        Self::new(MODELS_TTL, FRESH_TTL).persisted(
            workbench_core::paths::workbench_config_dir().join("models-cache.json"),
            |(_, cwd)| cwd.exists(),
        )
    }
}

impl ModelsCache {
    /// `account_id` must already be a known account (see `claude_accounts::resolve`)
    /// and `cwd` a session's, so request input can't grow the map. A stale
    /// list comes at once; `on_refresh` gets the newer one fetched behind it.
    pub async fn get(
        &self,
        account_id: Option<String>,
        cwd: PathBuf,
        on_refresh: OnRefresh<Vec<ModelOption>>,
    ) -> Result<Vec<ModelOption>> {
        let key = (account_id.clone(), cwd.clone());
        self.get_stale_with(
            key,
            move || claude_accounts::models(account_id.as_deref(), &cwd),
            on_refresh,
        )
        .await
    }

    /// A list fetched in the last few seconds, run now if there is none.
    pub async fn refresh(
        &self,
        account_id: Option<String>,
        cwd: PathBuf,
    ) -> Result<Vec<ModelOption>> {
        let key = (account_id.clone(), cwd.clone());
        self.get_with(key, true, move || {
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
            store: None,
            keep: |_| true,
        }
    }

    /// Keep good results in `file`, starting from what it holds that `keep`
    /// accepts and is younger than [`MODELS_MAX_AGE`].
    fn persisted(mut self, file: PathBuf, keep: fn(&K) -> bool) -> Self {
        let saved: Vec<Saved<K, T>> = workbench_core::paths::load_json(&file, Vec::new());
        let now = unix_now();
        let mut slots = lock(&self.slots);
        for entry in saved {
            let age = Duration::from_secs(now.saturating_sub(entry.saved_at));
            if age >= MODELS_MAX_AGE || !keep(&entry.key) {
                continue;
            }
            // Older than this process's clock reaches: due a refresh.
            let at = Instant::now()
                .checked_sub(age)
                .or_else(|| Instant::now().checked_sub(self.ttl))
                .unwrap_or_else(Instant::now);
            let slot = Slot::default();
            *lock(&slot.good) = Some((at, entry.value));
            slots.insert(entry.key, Arc::new(slot));
        }
        drop(slots);
        self.store = Some(Arc::new(Store {
            file,
            taken: AtomicU64::new(0),
            written: Mutex::new(0),
        }));
        self.keep = keep;
        self
    }

    /// Snapshot the good results now; `keep` (a stat per cwd, which can hang
    /// on a stale mount) and the write happen off the async thread.
    fn save(&self) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let now = unix_now();
        let saved: Vec<Saved<K, T>> = lock(&self.slots)
            .iter()
            .filter_map(|(key, slot)| {
                let (at, value) = slot.good()?;
                Some(Saved {
                    key: key.clone(),
                    saved_at: now.saturating_sub(at.elapsed().as_secs()),
                    value,
                })
            })
            .collect();
        let keep = self.keep;
        let taken = store.taken.fetch_add(1, Ordering::SeqCst) + 1;
        tokio::task::spawn_blocking(move || {
            let saved: Vec<_> = saved.into_iter().filter(|s| keep(&s.key)).collect();
            let content = match serde_json::to_string_pretty(&saved) {
                Ok(content) => content,
                Err(e) => return tracing::warn!("could not save the models cache: {e}"),
            };
            let mut written = lock(&store.written);
            if taken <= *written {
                return;
            }
            match workbench_core::paths::atomic_write(&store.file, &content) {
                Ok(()) => *written = taken,
                Err(e) => tracing::warn!("could not save {}: {e:#}", store.file.display()),
            }
        });
    }

    fn slot(&self, key: K) -> Arc<Slot<T>> {
        lock(&self.slots).entry(key).or_default().clone()
    }

    /// The result, run now unless one younger than the TTL is cached. A
    /// failed run answers with the last good result when there is one.
    async fn get_with<F>(&self, key: K, fresh: bool, fetch: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let slot = self.slot(key);
        let max = if fresh { self.fresh_ttl } else { self.ttl };
        if let Some(result) = slot.cached(max) {
            return result.map_err(anyhow::Error::msg);
        }
        // Concurrent requests wait for one run rather than start their own.
        let _run = slot.run.lock().await;
        if let Some(result) = slot.cached(max) {
            return result.map_err(anyhow::Error::msg);
        }
        match tokio::task::spawn_blocking(fetch).await? {
            Ok(value) => {
                *lock(&slot.good) = Some((Instant::now(), value.clone()));
                *lock(&slot.failed) = None;
                self.save();
                Ok(value)
            }
            Err(e) => {
                let e = format!("{e:#}");
                *lock(&slot.failed) = Some((Instant::now(), e.clone()));
                slot.good()
                    .map(|(_, v)| v)
                    .ok_or_else(|| anyhow::Error::msg(e))
            }
        }
    }

    /// A good result at once, however old, refreshed behind once past the
    /// TTL (`on_refresh` gets the newer one). Only a key with no good result
    /// waits for a run.
    async fn get_stale_with<F>(&self, key: K, fetch: F, on_refresh: OnRefresh<T>) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        match self.slot(key.clone()).good() {
            Some((at, value)) => {
                if at.elapsed() >= self.ttl {
                    self.refresh_behind(key, fetch, Some(on_refresh));
                }
                Ok(value)
            }
            None => self.get_with(key, false, fetch).await,
        }
    }

    /// The last good result, if any, never waiting; a run starts behind when
    /// there is none or it is older than `max`.
    fn peek_with<F>(&self, key: K, max: Duration, fetch: F) -> Option<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let good = self.slot(key.clone()).good();
        if good.as_ref().is_none_or(|(at, _)| at.elapsed() >= max) {
            self.refresh_behind(key, fetch, None);
        }
        good.map(|(_, value)| value)
    }

    fn refresh_behind<F>(&self, key: K, fetch: F, on_refresh: Option<OnRefresh<T>>)
    where
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let cache = self.clone();
        tokio::spawn(async move {
            match cache.get_with(key, false, fetch).await {
                Ok(value) => {
                    if let Some(done) = on_refresh {
                        done(value);
                    }
                }
                Err(e) => tracing::warn!("background refresh failed: {e:#}"),
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
    /// While a session of the account is linked, or for [`TTL`] after the last
    /// one reported; never past a reset of a window it reports.
    fn current(&self, live: bool) -> bool {
        let now = unix_now();
        (live || self.at.elapsed() < TTL)
            && self
                .limits
                .iter()
                .all(|l| l.resets_at.is_none_or(|reset| reset > now))
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
        lock(&self.readings).insert(account_id, reading);
    }

    fn reading(&self, account_id: &Option<String>, live: bool) -> Option<Vec<UsageLimit>> {
        let readings = lock(&self.readings);
        let reading = readings.get(account_id).filter(|r| r.current(live))?;
        Some(reading.limits.clone())
    }

    /// `account_id` must already be a known account (see `claude_accounts::resolve`),
    /// so request input can't grow the map. `live`: a session of that account
    /// is linked, so its reading stands however old.
    pub async fn get(
        &self,
        account_id: Option<String>,
        fresh: bool,
        live: bool,
    ) -> Result<Vec<UsageLimit>> {
        let id = account_id.clone();
        self.get_with(account_id, fresh, live, move || {
            claude_accounts::usage(id.as_deref())
        })
        .await
    }

    async fn get_with<F>(
        &self,
        account_id: Option<String>,
        fresh: bool,
        live: bool,
        probe: F,
    ) -> Result<Vec<UsageLimit>>
    where
        F: FnOnce() -> Result<Vec<UsageLimit>> + Send + 'static,
    {
        // A fresh request with no session running wants what `/usage` says now.
        let reading = if fresh && !live {
            None
        } else {
            self.reading(&account_id, live)
        };
        let Some(mut limits) = reading else {
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
    use std::sync::atomic::AtomicUsize;

    fn limit(percent: u8) -> Vec<UsageLimit> {
        vec![UsageLimit {
            label: "session".into(),
            percent,
            resets: None,
            resets_at: None,
        }]
    }

    type Probe = AccountCache<Option<String>, Vec<UsageLimit>>;

    fn probe() -> Probe {
        AccountCache::new(TTL, FRESH_TTL)
    }

    fn no_refresh<T>() -> OnRefresh<T> {
        Box::new(|_| {})
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
        let cache: Probe = AccountCache::new(TTL, Duration::from_millis(50));
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
    async fn a_failed_refresh_keeps_the_last_good_result() {
        let cache: Probe = AccountCache::new(Duration::from_millis(20), Duration::from_millis(20));
        cache.get_with(None, false, || Ok(limit(1))).await.unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        let kept = cache
            .get_with(None, false, || anyhow::bail!("timed out"))
            .await
            .unwrap();
        assert_eq!(kept, limit(1));
        let throttled = cache
            .get_with(None, false, || -> Result<_> {
                unreachable!("failed just now")
            })
            .await
            .unwrap();
        assert_eq!(throttled, limit(1), "no rerun within the failure TTL");
        assert_eq!(
            cache.peek_with(None, Duration::ZERO, || anyhow::bail!("again")),
            Some(limit(1))
        );
    }

    #[tokio::test]
    async fn an_expired_result_is_served_stale_and_refreshed_behind() {
        let cache: Probe = AccountCache::new(Duration::from_millis(50), Duration::from_millis(50));
        let first = cache
            .get_stale_with(None, || Ok(limit(1)), no_refresh())
            .await
            .unwrap();
        assert_eq!(first, limit(1), "a missing entry waits for its run");
        tokio::time::sleep(Duration::from_millis(60)).await;
        let slow = || {
            std::thread::sleep(Duration::from_millis(100));
            Ok(limit(2))
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        let started = Instant::now();
        let stale = cache
            .get_stale_with(
                None,
                slow,
                Box::new(move |v| {
                    let _ = tx.send(v);
                }),
            )
            .await
            .unwrap();
        assert_eq!(stale, limit(1), "past the TTL the old result comes at once");
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(rx.await.unwrap(), limit(2), "the refresh is handed on");
        let refreshed = cache
            .get_stale_with(
                None,
                || -> Result<_> { unreachable!("fresh again") },
                no_refresh(),
            )
            .await
            .unwrap();
        assert_eq!(refreshed, limit(2));
    }

    #[tokio::test]
    async fn good_results_outlive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cache.json");
        let key = (Some("work".to_string()), dir.path().to_path_buf());
        let gone = (None, dir.path().join("gone"));
        let keep: fn(&(Option<String>, PathBuf)) -> bool = |(_, cwd)| cwd.exists();
        let cache = AccountCache::new(TTL, TTL).persisted(file.clone(), keep);
        cache
            .get_stale_with(key.clone(), || Ok(limit(5)), no_refresh())
            .await
            .unwrap();
        std::fs::create_dir(&gone.1).unwrap();
        cache
            .get_stale_with(gone.clone(), || Ok(limit(7)), no_refresh())
            .await
            .unwrap();
        std::fs::remove_dir(&gone.1).unwrap();
        cache
            .get_stale_with(
                (None, PathBuf::from("/other")),
                || anyhow::bail!("no"),
                no_refresh(),
            )
            .await
            .unwrap_err();
        // The last save runs behind; wait for it to land.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let restarted: AccountCache<_, Vec<UsageLimit>> =
            AccountCache::new(TTL, TTL).persisted(file.clone(), keep);
        let kept = restarted
            .get_stale_with(
                key,
                || -> Result<_> { unreachable!("loaded from disk") },
                no_refresh(),
            )
            .await
            .unwrap();
        assert_eq!(kept, limit(5));
        let pruned = restarted
            .get_stale_with(gone, || Ok(limit(8)), no_refresh())
            .await
            .unwrap();
        assert_eq!(pruned, limit(8), "a cwd that's gone isn't kept");

        // Past the maximum age a saved list is ignored.
        let old =
            serde_json::json!([{ "key": [null, dir.path()], "savedAt": 0, "value": limit(9) }]);
        std::fs::write(&file, old.to_string()).unwrap();
        let ancient: AccountCache<_, Vec<UsageLimit>> =
            AccountCache::new(TTL, TTL).persisted(file, keep);
        let fetched = ancient
            .get_stale_with(
                (None, dir.path().to_path_buf()),
                || Ok(limit(10)),
                no_refresh(),
            )
            .await
            .unwrap();
        assert_eq!(fetched, limit(10));
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
        let work = || Some("work".to_string());
        let first = usage
            .get_with(work(), false, true, counted(runs.clone()))
            .await
            .unwrap();
        let labels: Vec<_> = first
            .iter()
            .map(|l| (l.label.as_str(), l.percent))
            .collect();
        assert_eq!(labels, [("session", 24), ("week (all models)", 40)]);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let second = usage
            .get_with(work(), true, true, counted(runs.clone()))
            .await
            .unwrap();
        assert_eq!(
            second.len(),
            3,
            "the probe's per-model limit joins once run"
        );
        assert_eq!(second[2].label, "week (Fable)");
        assert_eq!(runs.load(Ordering::SeqCst), 1, "run once, behind");

        // No session linked: a fresh request asks `/usage`.
        let unlinked = usage.get_with(work(), true, false, probed).await.unwrap();
        assert_eq!(unlinked[0].percent, 1);

        // The default login's reading reports a window that has reset since:
        // stale, so the probe answers.
        usage.note(None, &windows(90.0, unix_now() - 1));
        let default = usage.get_with(None, false, true, probed).await.unwrap();
        assert_eq!(default[0].percent, 1);
    }

    #[test]
    fn a_reading_stands_a_minute_past_its_session() {
        let reading = Reading {
            at: Instant::now() - TTL - Duration::from_secs(1),
            limits: limit(5),
        };
        assert!(reading.current(true));
        assert!(!reading.current(false));
    }
}
