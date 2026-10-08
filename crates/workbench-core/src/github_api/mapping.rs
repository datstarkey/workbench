//! Maps API responses onto the JSON the `gh` CLI prints, so `github.rs`'s parsers
//! read both paths unchanged. gh exports Go structs: a null string is `""` and a
//! null time is Go's zero time, and a `StatusContext` keeps only the fields below.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::types::GitHubWorkflowRun;

/// gh's `pr list --state all --limit 100` query, trimmed to the fields `github.rs` reads.
const PR_QUERY: &str = r#"query($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    pullRequests(first: 100, orderBy: {field: CREATED_AT, direction: DESC}) {
      nodes {
        number title state url isDraft headRefName reviewDecision mergeStateStatus
        commits(last: 1) { nodes { commit { statusCheckRollup { contexts(first: 100) { nodes {
          __typename
          ... on StatusContext { context state targetUrl createdAt }
          ... on CheckRun {
            name status conclusion startedAt completedAt detailsUrl
            checkSuite { workflowRun { workflow { name } } }
          }
        } } } } } }
      }
    }
  }
}"#;

const GO_ZERO_TIME: &str = "0001-01-01T00:00:00Z";

pub(super) fn pr_query(owner: &str, repo: &str) -> Value {
    json!({ "query": PR_QUERY, "variables": { "owner": owner, "name": repo } })
}

/// A GraphQL response → `gh pr list --json …` output. Any `errors` fail it, as in gh.
pub(super) fn prs_to_gh_shape(body: &Value) -> Result<Vec<Value>, String> {
    if let Some(errors) = body.get("errors").and_then(Value::as_array) {
        if !errors.is_empty() {
            let messages: Vec<&str> = errors
                .iter()
                .filter_map(|e| e.get("message").and_then(Value::as_str))
                .collect();
            return Err(messages.join("; "));
        }
    }
    let nodes = body
        .pointer("/data/repository/pullRequests/nodes")
        .and_then(Value::as_array)
        .ok_or("no repository in the response")?;
    Ok(nodes.iter().map(pr_to_gh_shape).collect())
}

fn pr_to_gh_shape(pr: &Value) -> Value {
    // gh exports the head commit's contexts (`[]` when it has no rollup) and `null`
    // only when the PR has no commit at all.
    let rollup = pr
        .pointer("/commits/nodes/0/commit")
        .map(|commit| {
            commit
                .pointer("/statusCheckRollup/contexts/nodes")
                .and_then(Value::as_array)
                .map(|nodes| nodes.iter().map(check_to_gh_shape).collect())
                .unwrap_or_default()
        })
        .map_or(Value::Null, Value::Array);

    json!({
        "number": pr["number"],
        "title": string(&pr["title"]),
        "state": string(&pr["state"]),
        "url": string(&pr["url"]),
        "isDraft": pr["isDraft"].as_bool().unwrap_or(false),
        "headRefName": string(&pr["headRefName"]),
        "reviewDecision": string(&pr["reviewDecision"]),
        "mergeStateStatus": string(&pr["mergeStateStatus"]),
        "statusCheckRollup": rollup,
    })
}

fn check_to_gh_shape(node: &Value) -> Value {
    let typename = string(&node["__typename"]);
    if typename == "CheckRun" {
        json!({
            "__typename": typename,
            "name": string(&node["name"]),
            "workflowName": string(&node["checkSuite"]["workflowRun"]["workflow"]["name"]),
            "status": string(&node["status"]),
            "conclusion": string(&node["conclusion"]),
            "startedAt": time(&node["startedAt"]),
            "completedAt": time(&node["completedAt"]),
            "detailsUrl": string(&node["detailsUrl"]),
        })
    } else {
        json!({
            "__typename": typename,
            "context": string(&node["context"]),
            "state": string(&node["state"]),
            "targetUrl": string(&node["targetUrl"]),
            "startedAt": time(&node["createdAt"]),
        })
    }
}

fn string(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn time(v: &Value) -> &str {
    v.as_str().unwrap_or(GO_ZERO_TIME)
}

#[derive(Deserialize)]
struct RestRuns {
    workflow_runs: Vec<RestRun>,
}

#[derive(Deserialize)]
struct RestRun {
    id: u64,
    name: Option<String>,
    display_title: Option<String>,
    head_branch: Option<String>,
    status: Option<String>,
    conclusion: Option<String>,
    html_url: Option<String>,
    event: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

/// `GET /repos/{o}/{r}/actions/runs` → what `gh run list --json …` deserializes to.
pub(super) fn rest_runs(body: &[u8]) -> Result<Vec<GitHubWorkflowRun>, serde_json::Error> {
    let page: RestRuns = serde_json::from_slice(body)?;
    Ok(page
        .workflow_runs
        .into_iter()
        .map(|r| GitHubWorkflowRun {
            id: r.id,
            name: r.name.unwrap_or_default(),
            display_title: r.display_title.unwrap_or_default(),
            head_branch: r.head_branch.unwrap_or_default(),
            status: r.status.unwrap_or_default(),
            conclusion: Some(r.conclusion.unwrap_or_default()),
            url: r.html_url.unwrap_or_default(),
            event: r.event.unwrap_or_default(),
            created_at: r.created_at.unwrap_or_default(),
            updated_at: r.updated_at.unwrap_or_default(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{group_runs_by_branch, prs_with_checks};

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/src/github_api/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn bytes(v: &Value) -> Vec<u8> {
        serde_json::to_vec(v).unwrap()
    }

    #[test]
    fn graphql_prs_map_to_the_gh_cli_output() {
        let mapped = prs_to_gh_shape(&fixture("graphql_prs.json")).unwrap();
        let gh = fixture("gh_pr_list.json");
        assert_eq!(Value::Array(mapped.clone()), gh);

        // Through the shared parsers, both give the same status.
        let (api_prs, api_checks) = prs_with_checks(&mapped).unwrap();
        let (gh_prs, gh_checks) = prs_with_checks(gh.as_array().unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value((&api_prs, &api_checks)).unwrap(),
            serde_json::to_value((&gh_prs, &gh_checks)).unwrap()
        );
    }

    #[test]
    fn graphql_prs_parse_to_the_expected_status() {
        let mapped = prs_to_gh_shape(&fixture("graphql_prs.json")).unwrap();
        let (prs, checks) = prs_with_checks(&mapped).unwrap();
        let pr = |n: u64| prs.iter().find(|p| p.number == n).unwrap();

        let passing = pr(10);
        assert_eq!(passing.checks_status.overall, "success");
        assert!(passing.actions.can_merge);
        assert_eq!(passing.review_decision.as_deref(), Some("APPROVED"));

        let failing = pr(11);
        assert_eq!(failing.checks_status.overall, "failure");
        assert_eq!(
            (failing.checks_status.passing, failing.checks_status.failing),
            (1, 1)
        );
        assert!(!failing.actions.can_merge);

        let pending = pr(12);
        assert_eq!(pending.checks_status.overall, "pending");
        assert_eq!(pending.checks_status.pending, 2);
        let pending_checks = &checks[&12];
        assert!(pending_checks.iter().all(|c| c.bucket == "pending"));
        let ci = pending_checks
            .iter()
            .find(|c| c.name == "ci/legacy")
            .unwrap();
        assert_eq!(ci.link, "https://ci.example.com/1");

        let draft = pr(13);
        assert!(draft.is_draft && draft.actions.can_mark_ready && !draft.actions.can_merge);
        assert_eq!(draft.checks_status.overall, "none");
        assert!(checks[&13].is_empty());

        let behind = pr(14);
        assert_eq!(behind.merge_state_status.as_deref(), Some("BEHIND"));
        assert!(behind.actions.can_update_branch);

        let merged = pr(9);
        assert_eq!(merged.state, "MERGED");
        assert!(!checks.contains_key(&9), "only open PRs carry check detail");
    }

    #[test]
    fn an_empty_repo_has_no_prs() {
        assert!(prs_to_gh_shape(&fixture("graphql_empty.json"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn graphql_errors_fail_the_read() {
        let body =
            json!({ "data": null, "errors": [{ "message": "Could not resolve to a Repository" }] });
        assert_eq!(
            prs_to_gh_shape(&body).unwrap_err(),
            "Could not resolve to a Repository"
        );
        assert!(prs_to_gh_shape(&json!({ "data": { "repository": null } })).is_err());
    }

    #[test]
    fn rest_runs_map_to_the_gh_cli_output() {
        let api = rest_runs(&bytes(&fixture("rest_runs.json"))).unwrap();
        let gh: Vec<GitHubWorkflowRun> =
            serde_json::from_value(fixture("gh_run_list.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&api).unwrap(),
            serde_json::to_value(&gh).unwrap()
        );

        let grouped = group_runs_by_branch(api);
        assert_eq!(grouped.len(), 3);
        assert_eq!(grouped["main"].runs.len(), 2, "latest run per workflow");
        assert_eq!(grouped["main"].status.overall, "failure");
        assert_eq!(grouped["feature"].status.overall, "pending");
        assert_eq!(grouped["release"].status.overall, "success");
    }

    #[test]
    fn rest_runs_of_an_empty_repo() {
        let body = bytes(&json!({ "total_count": 0, "workflow_runs": [] }));
        assert!(rest_runs(&body).unwrap().is_empty());
    }
}
