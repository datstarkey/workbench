//! The `gh` login's token per host, kept in memory only and never logged.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use super::{lock, ApiError};

/// Re-read now and then so `gh auth switch` / `gh auth login` reach the poll
/// without a restart; a failed read is kept a while so a host gh has no login for
/// doesn't spawn `gh auth token` on every poll.
const TOKEN_TTL: Duration = Duration::from_secs(10 * 60);
const FAILED_READ_TTL: Duration = Duration::from_secs(5 * 60);

struct Entry {
    read_at: Instant,
    token: Result<String, String>,
}

#[derive(Default)]
pub(super) struct TokenCache(Mutex<HashMap<String, Entry>>);

pub(super) static TOKENS: LazyLock<TokenCache> = LazyLock::new(TokenCache::default);

impl TokenCache {
    /// The lock is held across a read so concurrent polls spawn one `gh`, not one
    /// each; the read is bounded by `GH_READ_TIMEOUT`.
    fn get(
        &self,
        host: &str,
        read: impl Fn(&str) -> Result<String, ApiError>,
    ) -> Result<String, ApiError> {
        let mut tokens = lock(&self.0);
        if let Some(entry) = tokens.get(host) {
            let ttl = if entry.token.is_ok() {
                TOKEN_TTL
            } else {
                FAILED_READ_TTL
            };
            if entry.read_at.elapsed() < ttl {
                return entry.token.clone().map_err(ApiError::Token);
            }
        }
        let token = read(host);
        tokens.insert(
            host.to_string(),
            Entry {
                read_at: Instant::now(),
                token: match &token {
                    Ok(t) => Ok(t.clone()),
                    Err(ApiError::Token(e)) => Err(e.clone()),
                    Err(e) => Err(e.to_string()),
                },
            },
        );
        token
    }

    /// Drops `bad` unless another caller already replaced it.
    fn invalidate(&self, host: &str, bad: &str) {
        let mut tokens = lock(&self.0);
        if tokens.get(host).and_then(|e| e.token.as_deref().ok()) == Some(bad) {
            tokens.remove(host);
        }
    }
}

/// Calls `call` with the cached token; on a 401 re-reads the token once and retries.
pub(super) fn with_token<T>(
    cache: &TokenCache,
    host: &str,
    read: impl Fn(&str) -> Result<String, ApiError>,
    mut call: impl FnMut(&str) -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    let token = cache.get(host, &read)?;
    match call(&token) {
        Err(ApiError::Unauthorized) => {
            cache.invalidate(host, &token);
            call(&cache.get(host, &read)?)
        }
        result => result,
    }
}

pub(super) fn read_gh_token(host: &str) -> Result<String, ApiError> {
    let output = crate::shell::run_with_timeout(
        crate::shell::tool("gh")
            .args(["auth", "token", "--hostname", host])
            .current_dir(dirs::home_dir().unwrap_or_default()),
        crate::github::GH_READ_TIMEOUT,
    )
    .map_err(|e| ApiError::Token(e.to_string()))?;
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || token.is_empty() {
        return Err(ApiError::Token(format!(
            "`gh auth token` exited with {}",
            output.status
        )));
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn counting(reads: &AtomicUsize) -> impl Fn(&str) -> Result<String, ApiError> + '_ {
        |_| Ok(format!("t{}", reads.fetch_add(1, Ordering::SeqCst)))
    }

    #[test]
    fn a_401_rereads_the_token_once() {
        let cache = TokenCache::default();
        let reads = AtomicUsize::new(0);
        let mut seen = vec![];

        let result = with_token(&cache, "github.com", counting(&reads), |token| {
            seen.push(token.to_string());
            if token == "t0" {
                Err(ApiError::Unauthorized)
            } else {
                Ok(token.to_string())
            }
        });

        assert_eq!(result.unwrap(), "t1");
        assert_eq!(seen, ["t0", "t1"]);
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_second_401_is_returned_not_retried_again() {
        let cache = TokenCache::default();
        let reads = AtomicUsize::new(0);
        let mut calls = 0;

        let result: Result<(), _> = with_token(&cache, "github.com", counting(&reads), |_| {
            calls += 1;
            Err(ApiError::Unauthorized)
        });

        assert!(matches!(result, Err(ApiError::Unauthorized)));
        assert_eq!((calls, reads.load(Ordering::SeqCst)), (2, 2));
    }

    #[test]
    fn the_token_is_read_once_and_cached_per_host() {
        let cache = TokenCache::default();
        let reads = AtomicUsize::new(0);
        for _ in 0..3 {
            with_token(&cache, "github.com", counting(&reads), |_| Ok(())).unwrap();
        }
        with_token(&cache, "github.corp", counting(&reads), |_| Ok(())).unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_failed_read_is_not_retried_on_every_poll() {
        let cache = TokenCache::default();
        let reads = AtomicUsize::new(0);
        let read = |_: &str| {
            reads.fetch_add(1, Ordering::SeqCst);
            Err(ApiError::Token("not logged in".into()))
        };
        for _ in 0..3 {
            let result: Result<(), _> = with_token(&cache, "github.com", read, |_| Ok(()));
            assert!(matches!(result, Err(ApiError::Token(_))));
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}
