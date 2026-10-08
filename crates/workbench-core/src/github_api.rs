//! The GitHub poll's read path over the GitHub API instead of `gh` processes: one
//! GraphQL query for PRs and their checks, and ETag-cached REST pages for workflow
//! runs (a 304 doesn't count against the rate limit). Auth reuses the `gh` login
//! (`gh auth token`, read once per host and kept in memory only). Callers fall back
//! to the `gh` CLI on any error; mutations stay on the CLI.

mod mapping;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;

use crate::types::{GitHubRemote, GitHubWorkflowRun};

/// `gh run list --limit 200`, fetched the way gh does: pages of 100.
const RUNS_PER_PAGE: usize = 100;
const RUNS_PAGES: usize = 2;

#[derive(Debug)]
pub(crate) enum ApiError {
    Token(String),
    Unauthorized,
    Status(u16),
    Network(String),
    GraphQl(String),
    Parse(String),
}

impl ApiError {
    fn kind(&self) -> &'static str {
        match self {
            Self::Token(_) => "token",
            Self::Unauthorized => "unauthorized",
            Self::Status(_) => "status",
            Self::Network(_) => "network",
            Self::GraphQl(_) => "graphql",
            Self::Parse(_) => "parse",
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Token(e) => write!(f, "no gh token: {e}"),
            Self::Unauthorized => write!(f, "unauthorized (401)"),
            Self::Status(code) => write!(f, "HTTP {code}"),
            Self::Network(e) => write!(f, "network error: {e}"),
            Self::GraphQl(e) => write!(f, "GraphQL error: {e}"),
            Self::Parse(e) => write!(f, "unexpected response: {e}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Endpoints {
    graphql: String,
    rest: String,
    /// What `gh auth token --hostname` takes: the host without a port.
    token_host: String,
}

impl Endpoints {
    fn for_host(host: &str) -> Self {
        let token_host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
        if token_host == "github.com" {
            Self {
                graphql: "https://api.github.com/graphql".into(),
                rest: "https://api.github.com".into(),
                token_host,
            }
        } else {
            Self {
                graphql: format!("https://{host}/api/graphql"),
                rest: format!("https://{host}/api/v3"),
                token_host,
            }
        }
    }
}

/// Open PRs and recent closed/merged ones, in `gh pr list --state all --json …`'s shape.
pub(crate) fn fetch_pr_json(
    path: &str,
    host: &str,
    remote: &GitHubRemote,
) -> Result<Vec<Value>, ApiError> {
    let ep = Endpoints::for_host(host);
    let result = with_token(&TOKENS, &ep.token_host, read_gh_token, |token| {
        let (url, token) = (ep.graphql.clone(), token.to_string());
        let (owner, repo) = (remote.owner.clone(), remote.repo.clone());
        block_on(async move {
            let body = mapping::pr_query(&owner, &repo);
            let resp = authorized(client().post(url), &token)
                .header("Accept", "application/vnd.github.merge-info-preview+json")
                .json(&body)
                .send()
                .await
                .map_err(network)?;
            let resp = check_status(resp)?;
            let body: Value = resp.json().await.map_err(parse)?;
            mapping::prs_to_gh_shape(&body).map_err(ApiError::GraphQl)
        })?
    });
    report(path, "PR", &result);
    result
}

/// The latest 200 workflow runs, as `gh run list --json …` would return them.
pub(crate) fn fetch_workflow_runs(
    path: &str,
    host: &str,
    remote: &GitHubRemote,
) -> Result<Vec<GitHubWorkflowRun>, ApiError> {
    let ep = Endpoints::for_host(host);
    let base = format!(
        "{}/repos/{}/{}/actions/runs?per_page={RUNS_PER_PAGE}",
        ep.rest, remote.owner, remote.repo
    );
    let result = with_token(&TOKENS, &ep.token_host, read_gh_token, |token| {
        let (base, token) = (base.clone(), token.to_string());
        block_on(async move {
            let mut runs = Vec::new();
            for page in 1..=RUNS_PAGES {
                let page_runs = runs_page(&format!("{base}&page={page}"), &token).await?;
                let full = page_runs.len() == RUNS_PER_PAGE;
                runs.extend(page_runs);
                if !full {
                    break;
                }
            }
            Ok(runs)
        })?
    });
    report(path, "workflow runs", &result);
    result
}

/// Run pages by URL: their ETag and the runs it validates.
type EtagCache = HashMap<String, (String, Vec<GitHubWorkflowRun>)>;

/// One page of runs, revalidated with its ETag so an unchanged page costs a 304.
async fn runs_page(url: &str, token: &str) -> Result<Vec<GitHubWorkflowRun>, ApiError> {
    static ETAGS: OnceLock<Mutex<EtagCache>> = OnceLock::new();
    let etags = ETAGS.get_or_init(Default::default);
    let cached_etag = lock(etags).get(url).map(|(etag, _)| etag.clone());

    let mut req =
        authorized(client().get(url), token).header("Accept", "application/vnd.github+json");
    if let Some(etag) = &cached_etag {
        req = req.header("If-None-Match", etag);
    }
    let resp = req.send().await.map_err(network)?;
    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        if let Some((_, runs)) = lock(etags).get(url) {
            return Ok(runs.clone());
        }
    }
    let resp = check_status(resp)?;
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let body = resp.bytes().await.map_err(network)?;
    let runs = mapping::rest_runs(&body).map_err(parse)?;
    if let Some(etag) = etag {
        lock(etags).insert(url.to_string(), (etag, runs.clone()));
    }
    Ok(runs)
}

fn authorized(req: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    req.bearer_auth(token)
        .header("X-GitHub-Api-Version", "2022-11-28")
}

fn check_status(resp: reqwest::Response) -> Result<reqwest::Response, ApiError> {
    match resp.status() {
        reqwest::StatusCode::UNAUTHORIZED => Err(ApiError::Unauthorized),
        s if s.is_success() => Ok(resp),
        s => Err(ApiError::Status(s.as_u16())),
    }
}

fn network(e: reqwest::Error) -> ApiError {
    ApiError::Network(e.without_url().to_string())
}

fn parse(e: impl fmt::Display) -> ApiError {
    ApiError::Parse(e.to_string())
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(concat!("workbench/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default()
    })
}

/// Runs `fut` on this module's own runtime thread and waits for it. The callers are
/// sync and may themselves be on a Tokio worker (a Tauri `command(async)`), where a
/// nested `block_on` would panic; a channel wait has no such rule. One long-lived
/// runtime also keeps the client's connection pool warm across polls.
fn block_on<T: Send + 'static>(
    fut: impl Future<Output = T> + Send + 'static,
) -> Result<T, ApiError> {
    static HANDLE: OnceLock<Option<tokio::runtime::Handle>> = OnceLock::new();
    let handle = HANDLE.get_or_init(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        let handle = rt.handle().clone();
        std::thread::Builder::new()
            .name("github-api".into())
            .spawn(move || rt.block_on(std::future::pending::<()>()))
            .ok()?;
        Some(handle)
    });
    let handle = handle
        .as_ref()
        .ok_or_else(|| ApiError::Network("no runtime".into()))?;
    let (tx, rx) = std::sync::mpsc::channel();
    handle.spawn(async move {
        let _ = tx.send(fut.await);
    });
    rx.recv()
        .map_err(|_| ApiError::Network("request dropped".into()))
}

/// `gh` tokens by host. Never logged or written anywhere.
struct TokenCache(Mutex<HashMap<String, String>>);

static TOKENS: LazyLock<TokenCache> = LazyLock::new(|| TokenCache(Mutex::default()));

impl TokenCache {
    /// The lock is held across a read so concurrent first polls spawn one `gh`, not one each.
    fn get(
        &self,
        host: &str,
        read: impl Fn(&str) -> Result<String, ApiError>,
    ) -> Result<String, ApiError> {
        let mut tokens = lock(&self.0);
        if let Some(token) = tokens.get(host) {
            return Ok(token.clone());
        }
        let token = read(host)?;
        tokens.insert(host.to_string(), token.clone());
        Ok(token)
    }

    /// Drops `bad` unless another caller already replaced it.
    fn invalidate(&self, host: &str, bad: &str) {
        let mut tokens = lock(&self.0);
        if tokens.get(host).map(String::as_str) == Some(bad) {
            tokens.remove(host);
        }
    }
}

/// Calls `call` with the cached token; on a 401 re-reads the token once and retries.
fn with_token<T>(
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

fn read_gh_token(host: &str) -> Result<String, ApiError> {
    let output = crate::shell::tool("gh")
        .args(["auth", "token", "--hostname", host])
        .current_dir(dirs::home_dir().unwrap_or_default())
        .output()
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

/// (project path, part, failure kind) already logged.
type Warned = (String, &'static str, &'static str);

/// Logs a fallback once per project, part and failure kind; a success re-arms it.
fn report<T>(path: &str, part: &'static str, result: &Result<T, ApiError>) {
    static WARNED: OnceLock<Mutex<HashSet<Warned>>> = OnceLock::new();
    let mut warned = lock(WARNED.get_or_init(Default::default));
    match result {
        Ok(_) => warned.retain(|(p, w, _)| !(p == path && *w == part)),
        Err(e) => {
            if warned.insert((path.to_string(), part, e.kind())) {
                log::warn!("[github] API {part} read failed for {path} ({e}); using the gh CLI");
            }
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn github_com_uses_the_api_host() {
        let ep = Endpoints::for_host("github.com");
        assert_eq!(ep.graphql, "https://api.github.com/graphql");
        assert_eq!(ep.rest, "https://api.github.com");
        assert_eq!(ep.token_host, "github.com");
    }

    #[test]
    fn enterprise_hosts_use_their_api_paths() {
        let ep = Endpoints::for_host("github.mycompany.com");
        assert_eq!(ep.graphql, "https://github.mycompany.com/api/graphql");
        assert_eq!(ep.rest, "https://github.mycompany.com/api/v3");
        assert_eq!(ep.token_host, "github.mycompany.com");

        let ep = Endpoints::for_host("github.corp:8443");
        assert_eq!(ep.graphql, "https://github.corp:8443/api/graphql");
        assert_eq!(ep.token_host, "github.corp");
    }

    #[test]
    fn a_401_rereads_the_token_once() {
        let cache = TokenCache(Mutex::new(HashMap::new()));
        let reads = AtomicUsize::new(0);
        let read = |_: &str| Ok(format!("t{}", reads.fetch_add(1, Ordering::SeqCst)));
        let mut seen = vec![];

        let result = with_token(&cache, "github.com", read, |token| {
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
        let cache = TokenCache(Mutex::new(HashMap::new()));
        let reads = AtomicUsize::new(0);
        let read = |_: &str| Ok(format!("t{}", reads.fetch_add(1, Ordering::SeqCst)));
        let mut calls = 0;

        let result: Result<(), _> = with_token(&cache, "github.com", read, |_| {
            calls += 1;
            Err(ApiError::Unauthorized)
        });

        assert!(matches!(result, Err(ApiError::Unauthorized)));
        assert_eq!((calls, reads.load(Ordering::SeqCst)), (2, 2));
    }

    #[test]
    fn the_token_is_read_once_and_cached() {
        let cache = TokenCache(Mutex::new(HashMap::new()));
        let reads = AtomicUsize::new(0);
        let read = |_: &str| {
            reads.fetch_add(1, Ordering::SeqCst);
            Ok("tok".to_string())
        };
        for _ in 0..3 {
            with_token(&cache, "github.com", read, |_| Ok(())).unwrap();
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn block_on_works_from_inside_a_tokio_runtime() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let value = rt.block_on(async { block_on(async { 7 }).unwrap() });
        assert_eq!(value, 7);
    }

    /// Read-only smoke test against the real API with the local `gh` login:
    /// `cargo test -p workbench-core github_api -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_api_matches_the_gh_cli() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let (remote, host) = crate::github::github_remote_and_host(path).unwrap();

        let api = fetch_pr_json(path, &host, &remote).expect("API PR read");
        let cli = crate::github::fetch_pr_json(path).unwrap();
        println!("{} PRs via the API", api.len());
        assert_eq!(api.len(), cli.len());
        // An open PR's checks can move between the two reads; compare their shape.
        let shape = |pr: &Value| {
            let mut pr = pr.clone();
            for node in pr["statusCheckRollup"].as_array_mut().into_iter().flatten() {
                for key in ["status", "conclusion", "state", "startedAt", "completedAt"] {
                    if let Some(v) = node.get_mut(key) {
                        *v = Value::Null;
                    }
                }
            }
            pr
        };
        for (a, c) in api.iter().zip(&cli) {
            if c["state"] == "OPEN" {
                assert_eq!(shape(a), shape(c), "PR #{} differs", c["number"]);
            } else {
                assert_eq!(a, c, "PR #{} differs", c["number"]);
            }
        }

        let api = fetch_workflow_runs(path, &host, &remote).expect("API runs read");
        let cli = crate::github::list_workflow_runs(path);
        println!("{} workflow runs via the API", api.len());
        assert_eq!(
            serde_json::to_value(&api).unwrap(),
            serde_json::to_value(&cli).unwrap()
        );
        // A second read revalidates with the ETag.
        assert_eq!(
            fetch_workflow_runs(path, &host, &remote).unwrap().len(),
            api.len()
        );
    }
}
