//! The GitHub poll's read path over the GitHub API instead of `gh` processes: one
//! GraphQL query for PRs and their checks, and ETag-cached REST pages for workflow
//! runs (a 304 doesn't count against the rate limit). Auth reuses the `gh` login
//! (`token.rs`). Callers fall back to the `gh` CLI on any error, and a failing
//! project skips the API for a while; mutations stay on the CLI.

mod mapping;
mod token;

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::github::GitRemote;
use crate::types::{GitHubRemote, GitHubWorkflowRun};
use token::{read_gh_token, with_token, TOKENS};

/// `gh run list --limit 200`, fetched the way gh does: pages of 100.
const RUNS_PER_PAGE: usize = 100;
const RUNS_PAGES: usize = 2;

/// After a failure a project's part goes straight to the CLI for this long, so a
/// broken API path (unreachable host, no access) doesn't cost a round trip per poll.
const BACKOFF: Duration = Duration::from_secs(5 * 60);

#[derive(Debug)]
pub(crate) enum ApiError {
    Token(String),
    Unauthorized,
    Status(u16),
    Network(String),
    GraphQl(String),
    Parse(String),
    BackedOff,
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
            Self::BackedOff => "backed off",
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
            Self::BackedOff => write!(f, "backing off after a failure"),
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

/// The repo `gh pr list` / `gh run list` would read, when that's unambiguous: the
/// only GitHub repo among the remotes, or the remote `gh repo set-default` picked.
/// `None` leaves the choice to the CLI (e.g. a fork with `upstream` and no default).
pub(crate) fn api_remote(path: &str, remotes: &[GitRemote]) -> Option<GitRemote> {
    let first = remotes.first()?;
    if remotes.iter().all(|r| {
        r.remote
            .html_url
            .eq_ignore_ascii_case(&first.remote.html_url)
    }) {
        return Some(first.clone());
    }
    let config = crate::git::git_output(
        &["config", "--get-regexp", r"^remote\..*\.gh-resolved$"],
        path,
    )
    .ok()?;
    resolved_remote(&config, remotes)
}

/// `remote.<name>.gh-resolved` is `base` (that remote is the repo) or, from older
/// gh releases, `owner/repo` on that remote's host.
fn resolved_remote(config: &str, remotes: &[GitRemote]) -> Option<GitRemote> {
    config.lines().find_map(|line| {
        let (key, value) = line.split_once(' ')?;
        let name = key.strip_prefix("remote.")?.strip_suffix(".gh-resolved")?;
        let base = remotes.iter().find(|r| r.name == name)?;
        match value.trim() {
            "base" => Some(base.clone()),
            other => {
                let (owner, repo) = other.split_once('/')?;
                Some(GitRemote {
                    name: base.name.clone(),
                    remote: GitHubRemote {
                        owner: owner.to_string(),
                        repo: repo.to_string(),
                        html_url: format!("https://{}/{owner}/{repo}", base.host),
                    },
                    host: base.host.clone(),
                })
            }
        }
    })
}

/// Open PRs and recent closed/merged ones, in `gh pr list --state all --json …`'s shape.
pub(crate) fn fetch_pr_json(path: &str, target: &GitRemote) -> Result<Vec<Value>, ApiError> {
    let ep = Endpoints::for_host(&target.host);
    guarded(path, "PR", || {
        with_token(&TOKENS, &ep.token_host, read_gh_token, |token| {
            let (url, token) = (ep.graphql.clone(), token.to_string());
            let body = mapping::pr_query(&target.remote.owner, &target.remote.repo);
            block_on(async move {
                let resp = authorized(client()?.post(url), &token)
                    .header("Accept", "application/vnd.github.merge-info-preview+json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(network)?;
                let body: Value = check_status(resp)?.json().await.map_err(parse)?;
                mapping::prs_to_gh_shape(&body).map_err(ApiError::GraphQl)
            })?
        })
    })
}

/// The latest 200 workflow runs, as `gh run list --json …` would return them.
pub(crate) fn fetch_workflow_runs(
    path: &str,
    target: &GitRemote,
) -> Result<Vec<GitHubWorkflowRun>, ApiError> {
    let ep = Endpoints::for_host(&target.host);
    let base = format!(
        "{}/repos/{}/{}/actions/runs?per_page={RUNS_PER_PAGE}",
        ep.rest, target.remote.owner, target.remote.repo
    );
    guarded(path, "workflow runs", || {
        with_token(&TOKENS, &ep.token_host, read_gh_token, |token| {
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
        })
    })
}

/// Run pages by URL: their ETag and the runs it validates.
type EtagCache = HashMap<String, (String, Vec<GitHubWorkflowRun>)>;

/// One page of runs, revalidated with its ETag so an unchanged page costs a 304.
async fn runs_page(url: &str, token: &str) -> Result<Vec<GitHubWorkflowRun>, ApiError> {
    static ETAGS: OnceLock<Mutex<EtagCache>> = OnceLock::new();
    let etags = ETAGS.get_or_init(Default::default);
    let cached_etag = lock(etags).get(url).map(|(etag, _)| etag.clone());

    let mut req =
        authorized(client()?.get(url), token).header("Accept", "application/vnd.github+json");
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

/// Shared, so its connection pool stays warm across polls; every request is bounded
/// by `http::REQUEST_TIMEOUT`.
fn client() -> Result<&'static reqwest::Client, ApiError> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            crate::http::client_builder(crate::http::REQUEST_TIMEOUT)
                .build()
                .ok()
        })
        .as_ref()
        .ok_or_else(|| ApiError::Network("no HTTP client".into()))
}

/// Runs `fut` on this module's own runtime thread and waits for it. The callers are
/// sync and may themselves be on a Tokio worker, where a nested `block_on` would
/// panic; a channel wait has no such rule. One long-lived runtime also keeps the
/// client's pooled connections usable (they die with the runtime that opened them).
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

struct Failure {
    at: Instant,
    kind: &'static str,
}

/// Last failure per (project path, part); cleared by a success.
static FAILURES: LazyLock<Mutex<HashMap<(String, &'static str), Failure>>> =
    LazyLock::new(Default::default);

/// Runs `read` unless this project's `part` failed within `BACKOFF`. Logs a
/// fallback once per project, part and failure kind.
fn guarded<T>(
    path: &str,
    part: &'static str,
    read: impl FnOnce() -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    let key = (path.to_string(), part);
    if lock(&FAILURES)
        .get(&key)
        .is_some_and(|f| f.at.elapsed() < BACKOFF)
    {
        return Err(ApiError::BackedOff);
    }
    let result = read();
    let mut failures = lock(&FAILURES);
    match &result {
        Ok(_) => {
            failures.remove(&key);
        }
        Err(e) => {
            let failure = Failure {
                at: Instant::now(),
                kind: e.kind(),
            };
            if failures.insert(key, failure).map(|f| f.kind) != Some(e.kind()) {
                log::warn!("[github] API {part} read failed for {path} ({e}); using the gh CLI");
            }
        }
    }
    result
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(name: &str, owner: &str, host: &str) -> GitRemote {
        GitRemote {
            name: name.into(),
            remote: GitHubRemote {
                owner: owner.into(),
                repo: "proj".into(),
                html_url: format!("https://{host}/{owner}/proj"),
            },
            host: host.into(),
        }
    }

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
    fn a_single_github_repo_is_the_api_target() {
        let remotes = [remote("origin", "me", "github.com")];
        assert_eq!(api_remote("/nowhere", &remotes), Some(remotes[0].clone()));
        assert_eq!(api_remote("/nowhere", &[]), None);
    }

    #[test]
    fn set_default_picks_the_target_among_several_remotes() {
        let remotes = [
            remote("origin", "me", "github.com"),
            remote("upstream", "org", "github.com"),
        ];
        let config = "remote.upstream.gh-resolved base\n";
        assert_eq!(resolved_remote(config, &remotes), Some(remotes[1].clone()));
        assert_eq!(resolved_remote("", &remotes), None);

        let legacy = resolved_remote("remote.origin.gh-resolved org/proj\n", &remotes).unwrap();
        assert_eq!(legacy.remote, remotes[1].remote);
    }

    #[test]
    fn a_failure_backs_off_until_a_success() {
        let path = "/test/backoff";
        let mut calls = 0;
        let fail = |calls: &mut i32| -> Result<(), ApiError> {
            *calls += 1;
            Err(ApiError::Status(502))
        };
        assert!(matches!(
            guarded(path, "PR", || fail(&mut calls)),
            Err(ApiError::Status(502))
        ));
        assert!(matches!(
            guarded(path, "PR", || fail(&mut calls)),
            Err(ApiError::BackedOff)
        ));
        assert_eq!(calls, 1);
        // Other parts and projects are unaffected.
        assert!(guarded(path, "workflow runs", || Ok(())).is_ok());

        lock(&FAILURES)
            .get_mut(&(path.to_string(), "PR"))
            .unwrap()
            .at -= BACKOFF;
        assert!(guarded(path, "PR", || Ok(())).is_ok());
        assert!(!lock(&FAILURES).contains_key(&(path.to_string(), "PR")));
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
        let remotes = crate::github::github_remotes(path).unwrap();
        let target = api_remote(path, &remotes).expect("one GitHub repo");

        let api = fetch_pr_json(path, &target).expect("API PR read");
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

        let api = fetch_workflow_runs(path, &target).expect("API runs read");
        let cli = crate::github::list_workflow_runs(path);
        println!("{} workflow runs via the API", api.len());
        assert_eq!(
            serde_json::to_value(&api).unwrap(),
            serde_json::to_value(&cli).unwrap()
        );
        // A second read revalidates with the ETag.
        assert_eq!(fetch_workflow_runs(path, &target).unwrap().len(), api.len());
    }
}
