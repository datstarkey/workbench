use std::collections::HashMap;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::types::{
    GitHubBranchRuns, GitHubCheckDetail, GitHubChecksStatus, GitHubPR, GitHubPRActions,
    GitHubProjectStatus, GitHubRemote, GitHubWorkflowRun,
};

fn gh_output(args: &[&str], cwd: &str) -> Result<String> {
    let output = Command::new("gh")
        .args(args)
        .current_dir(cwd)
        .env("PATH", crate::paths::enriched_path())
        .output()
        .context("Failed to run gh CLI")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("{stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn is_gh_available() -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    Command::new("gh")
        .args(["auth", "status"])
        .current_dir(home)
        .env("PATH", crate::paths::enriched_path())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn get_github_remote(path: &str) -> Result<GitHubRemote> {
    let url = crate::git::git_output(&["remote", "get-url", "origin"], path)?;
    parse_github_remote(&url)
}

fn parse_github_remote(url: &str) -> Result<GitHubRemote> {
    let (host, owner, repo) = parse_github_remote_parts(url)?;
    let html_url = format!("https://{host}/{owner}/{repo}");
    Ok(GitHubRemote {
        owner,
        repo,
        html_url,
    })
}

fn parse_github_remote_parts(url: &str) -> Result<(String, String, String)> {
    if let Some((prefix, rest)) = url.split_once(':') {
        if let Some(host) = prefix.split('@').nth(1) {
            if !is_supported_github_host(host) {
                bail!("Not a GitHub remote: {url}");
            }
            let rest = rest.trim_end_matches(".git");
            let parts: Vec<&str> = rest.splitn(2, '/').collect();
            if parts.len() != 2 {
                bail!("Cannot parse SSH remote: {url}");
            }
            return Ok((host.to_string(), parts[0].to_string(), parts[1].to_string()));
        }
    }

    if let Ok(parsed) = reqwest::Url::parse(url) {
        let Some(host) = parsed.host_str() else {
            bail!("Cannot parse remote host: {url}");
        };
        if !is_supported_github_host(host) {
            bail!("Not a GitHub remote: {url}");
        }
        let segments: Vec<&str> = parsed
            .path_segments()
            .map(|s| s.filter(|p| !p.is_empty()).collect())
            .unwrap_or_default();
        if segments.len() < 2 {
            bail!("Cannot parse HTTPS remote: {url}");
        }
        let owner = segments[0].to_string();
        let repo = segments[1].trim_end_matches(".git").to_string();
        return Ok((host.to_string(), owner, repo));
    }

    bail!("Cannot parse remote: {url}");
}

fn is_supported_github_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("github.com") {
        return true;
    }
    let host = host.to_ascii_lowercase();
    if host.starts_with("github.") || host.ends_with(".github.com") || host.contains(".github.") {
        return true;
    }
    if let Ok(gh_host) = std::env::var("GH_HOST") {
        if host.eq_ignore_ascii_case(&gh_host) {
            return true;
        }
    }
    if let Ok(github_host) = std::env::var("GITHUB_HOST") {
        if host.eq_ignore_ascii_case(&github_host) {
            return true;
        }
    }
    false
}

pub fn list_project_prs(path: &str) -> Result<Vec<GitHubPR>> {
    let fields = "number,title,state,url,isDraft,headRefName,reviewDecision,statusCheckRollup,mergeStateStatus";
    let result = gh_output(
        &[
            "pr", "list", "--state", "all", "--limit", "100", "--json", fields,
        ],
        path,
    );

    match result {
        Ok(json_str) => {
            let arr: Vec<serde_json::Value> = serde_json::from_str(&json_str)?;
            arr.iter().map(parse_pr_json).collect()
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not a git repository") || msg.contains("no GitHub remotes") {
                Ok(vec![])
            } else {
                Err(e)
            }
        }
    }
}

pub fn list_workflow_runs(path: &str) -> Vec<GitHubWorkflowRun> {
    let fields =
        "databaseId,name,displayTitle,headBranch,status,conclusion,url,event,createdAt,updatedAt";
    let result = gh_output(&["run", "list", "--limit", "200", "--json", fields], path);

    match result {
        Ok(json_str) => {
            if json_str.is_empty() {
                return vec![];
            }
            serde_json::from_str(&json_str).unwrap_or_default()
        }
        Err(_) => vec![],
    }
}

pub fn group_runs_by_branch(runs: Vec<GitHubWorkflowRun>) -> HashMap<String, GitHubBranchRuns> {
    let mut by_branch: HashMap<String, Vec<GitHubWorkflowRun>> = HashMap::new();
    for run in runs {
        by_branch
            .entry(run.head_branch.clone())
            .or_default()
            .push(run);
    }

    let mut result = HashMap::new();
    for (branch, mut branch_runs) in by_branch {
        // Sort by created_at descending so we can dedup by keeping the latest per workflow
        branch_runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Keep only the latest run per workflow name
        let mut seen_workflows: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let deduped: Vec<GitHubWorkflowRun> = branch_runs
            .into_iter()
            .filter(|r| seen_workflows.insert(r.name.clone()))
            .collect();

        let status = derive_branch_status(&deduped);
        result.insert(
            branch,
            GitHubBranchRuns {
                status,
                runs: deduped,
            },
        );
    }

    result
}

fn derive_overall_status(passing: u32, failing: u32, pending: u32) -> GitHubChecksStatus {
    let total = passing + failing + pending;
    let overall = if total == 0 {
        "none"
    } else if failing > 0 {
        "failure"
    } else if pending > 0 {
        "pending"
    } else {
        "success"
    };

    GitHubChecksStatus {
        overall: overall.to_string(),
        total,
        passing,
        failing,
        pending,
    }
}

fn derive_branch_status(runs: &[GitHubWorkflowRun]) -> GitHubChecksStatus {
    let mut passing = 0u32;
    let mut failing = 0u32;
    let mut pending = 0u32;

    for run in runs {
        if run.status == "completed" {
            match run.conclusion.as_deref() {
                Some("success") | Some("skipped") | Some("neutral") => passing += 1,
                Some("failure") | Some("cancelled") | Some("timed_out") => failing += 1,
                _ => pending += 1,
            }
        } else {
            pending += 1;
        }
    }

    derive_overall_status(passing, failing, pending)
}

pub fn get_project_status(path: &str) -> GitHubProjectStatus {
    let remote = get_github_remote(path).ok();
    let prs = if remote.is_some() {
        list_project_prs(path).unwrap_or_else(|e| {
            eprintln!("[github] Failed to list PRs for {path}: {e}");
            vec![]
        })
    } else {
        vec![]
    };

    let workflow_runs = if remote.is_some() {
        list_workflow_runs(path)
    } else {
        vec![]
    };
    let branch_runs = group_runs_by_branch(workflow_runs);

    // Pre-fetch checks for all open PRs
    let mut pr_checks: HashMap<u64, Vec<GitHubCheckDetail>> = HashMap::new();
    for pr in &prs {
        if pr.state == "OPEN" {
            if let Ok(checks) = list_pr_checks(path, pr.number) {
                pr_checks.insert(pr.number, checks);
            }
        }
    }

    GitHubProjectStatus {
        remote,
        prs,
        branch_runs,
        pr_checks,
    }
}

fn parse_pr_json(v: &serde_json::Value) -> Result<GitHubPR> {
    let checks = parse_checks_rollup(v.get("statusCheckRollup"));
    let state = v["state"].as_str().unwrap_or("OPEN").to_string();
    let is_draft = v["isDraft"].as_bool().unwrap_or(false);
    let merge_state_status = v["mergeStateStatus"].as_str().map(String::from);
    let actions = derive_pr_actions(&state, is_draft, merge_state_status.as_deref(), &checks);

    Ok(GitHubPR {
        number: v["number"].as_u64().unwrap_or(0),
        title: v["title"].as_str().unwrap_or("").to_string(),
        state,
        url: v["url"].as_str().unwrap_or("").to_string(),
        is_draft,
        head_ref_name: v["headRefName"].as_str().unwrap_or("").to_string(),
        review_decision: v["reviewDecision"].as_str().map(String::from),
        checks_status: checks,
        merge_state_status,
        actions,
    })
}

fn derive_pr_actions(
    state: &str,
    is_draft: bool,
    merge_state_status: Option<&str>,
    checks_status: &GitHubChecksStatus,
) -> GitHubPRActions {
    let is_open = state == "OPEN";
    let can_mark_ready = is_open && is_draft;
    let can_update_branch = is_open && merge_state_status == Some("BEHIND");
    let can_merge = is_open
        && !is_draft
        && merge_state_status != Some("DIRTY")
        && checks_status.failing == 0
        && checks_status.pending == 0;

    GitHubPRActions {
        can_merge,
        can_mark_ready,
        can_update_branch,
    }
}

fn parse_checks_rollup(rollup: Option<&serde_json::Value>) -> GitHubChecksStatus {
    let arr = match rollup.and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => a,
        _ => return derive_overall_status(0, 0, 0),
    };

    let mut passing = 0u32;
    let mut failing = 0u32;
    let mut pending = 0u32;

    for check in arr {
        let state = check
            .get("conclusion")
            .or_else(|| check.get("state"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match state.to_uppercase().as_str() {
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => passing += 1,
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STALE" => {
                failing += 1
            }
            _ => pending += 1,
        }
    }

    derive_overall_status(passing, failing, pending)
}

pub fn list_pr_checks(path: &str, pr_number: u64) -> Result<Vec<GitHubCheckDetail>> {
    let fields = "name,bucket,completedAt,startedAt,link,workflow,description";
    let result = gh_output(
        &["pr", "checks", &pr_number.to_string(), "--json", fields],
        path,
    );

    match result {
        Ok(json_str) => {
            if json_str.is_empty() {
                return Ok(vec![]);
            }
            let checks: Vec<GitHubCheckDetail> = serde_json::from_str(&json_str)?;
            Ok(checks)
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("no checks") || msg.contains("no pull requests") {
                Ok(vec![])
            } else {
                Err(e)
            }
        }
    }
}

pub fn update_pr_branch(path: &str, pr_number: u64) -> Result<()> {
    let remote = get_github_remote(path)?;
    gh_output(
        &[
            "api",
            &format!(
                "repos/{}/{}/pulls/{}/update-branch",
                remote.owner, remote.repo, pr_number
            ),
            "-X",
            "PUT",
        ],
        path,
    )?;
    Ok(())
}

pub fn rerun_workflow(path: &str, run_id: u64) -> Result<()> {
    gh_output(&["run", "rerun", &run_id.to_string(), "--failed"], path)?;
    Ok(())
}

pub fn mark_pr_ready(path: &str, pr_number: u64) -> Result<()> {
    gh_output(&["pr", "ready", &pr_number.to_string()], path)?;
    Ok(())
}

pub fn merge_pr(path: &str, pr_number: u64) -> Result<()> {
    gh_output(&["pr", "merge", &pr_number.to_string(), "--squash"], path)?;
    Ok(())
}

pub fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .context("Failed to open URL")?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .context("Failed to open URL")?;
    }
    #[cfg(target_os = "windows")]
    {
        // Empty title ("") prevents `start` from misinterpreting URLs with special chars
        Command::new("cmd")
            .args(["/c", "start", "\"\"", url])
            .spawn()
            .context("Failed to open URL")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ssh_remote() {
        let remote = parse_github_remote("git@github.com:user/repo.git").unwrap();
        assert_eq!(remote.owner, "user");
        assert_eq!(remote.repo, "repo");
        assert_eq!(remote.html_url, "https://github.com/user/repo");
    }

    #[test]
    fn parse_https_remote() {
        let remote = parse_github_remote("https://github.com/user/repo.git").unwrap();
        assert_eq!(remote.owner, "user");
        assert_eq!(remote.repo, "repo");
        assert_eq!(remote.html_url, "https://github.com/user/repo");
    }

    #[test]
    fn parse_https_remote_no_git_suffix() {
        let remote = parse_github_remote("https://github.com/user/repo").unwrap();
        assert_eq!(remote.owner, "user");
        assert_eq!(remote.repo, "repo");
    }

    #[test]
    fn parse_enterprise_https_remote() {
        let remote = parse_github_remote("https://github.mycompany.com/user/repo.git").unwrap();
        assert_eq!(remote.owner, "user");
        assert_eq!(remote.repo, "repo");
        assert_eq!(remote.html_url, "https://github.mycompany.com/user/repo");
    }

    #[test]
    fn parse_enterprise_ssh_remote() {
        let remote = parse_github_remote("git@github.mycompany.com:user/repo.git").unwrap();
        assert_eq!(remote.owner, "user");
        assert_eq!(remote.repo, "repo");
        assert_eq!(remote.html_url, "https://github.mycompany.com/user/repo");
    }

    #[test]
    fn parse_non_github_remote_fails() {
        assert!(parse_github_remote("https://gitlab.com/user/repo.git").is_err());
    }

    // derive_overall_status

    #[test]
    fn derive_overall_status_all_passing() {
        let s = derive_overall_status(3, 0, 0);
        assert_eq!(s.overall, "success");
        assert_eq!(s.total, 3);
        assert_eq!(s.passing, 3);
    }

    #[test]
    fn derive_overall_status_some_failing() {
        let s = derive_overall_status(2, 1, 0);
        assert_eq!(s.overall, "failure");
        assert_eq!(s.failing, 1);
    }

    #[test]
    fn derive_overall_status_some_pending() {
        let s = derive_overall_status(2, 0, 1);
        assert_eq!(s.overall, "pending");
        assert_eq!(s.pending, 1);
    }

    #[test]
    fn derive_overall_status_failing_beats_pending() {
        let s = derive_overall_status(1, 1, 1);
        assert_eq!(s.overall, "failure");
    }

    #[test]
    fn derive_overall_status_none_when_empty() {
        let s = derive_overall_status(0, 0, 0);
        assert_eq!(s.overall, "none");
        assert_eq!(s.total, 0);
    }

    // derive_branch_status

    fn make_run(status: &str, conclusion: Option<&str>) -> GitHubWorkflowRun {
        GitHubWorkflowRun {
            id: 1,
            name: "CI".to_string(),
            display_title: "test".to_string(),
            head_branch: "main".to_string(),
            status: status.to_string(),
            conclusion: conclusion.map(String::from),
            url: String::new(),
            event: "push".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn derive_branch_status_completed_success() {
        let runs = vec![make_run("completed", Some("success"))];
        let s = derive_branch_status(&runs);
        assert_eq!(s.overall, "success");
        assert_eq!(s.passing, 1);
    }

    #[test]
    fn derive_branch_status_completed_skipped_counts_as_passing() {
        let runs = vec![make_run("completed", Some("skipped"))];
        let s = derive_branch_status(&runs);
        assert_eq!(s.passing, 1);
    }

    #[test]
    fn derive_branch_status_completed_failure() {
        let runs = vec![make_run("completed", Some("failure"))];
        let s = derive_branch_status(&runs);
        assert_eq!(s.overall, "failure");
        assert_eq!(s.failing, 1);
    }

    #[test]
    fn derive_branch_status_in_progress_is_pending() {
        let runs = vec![make_run("in_progress", None)];
        let s = derive_branch_status(&runs);
        assert_eq!(s.overall, "pending");
        assert_eq!(s.pending, 1);
    }

    #[test]
    fn derive_branch_status_mixed() {
        let runs = vec![
            make_run("completed", Some("success")),
            make_run("completed", Some("failure")),
            make_run("in_progress", None),
        ];
        let s = derive_branch_status(&runs);
        assert_eq!(s.overall, "failure");
        assert_eq!(s.total, 3);
        assert_eq!(s.passing, 1);
        assert_eq!(s.failing, 1);
        assert_eq!(s.pending, 1);
    }

    #[test]
    fn derive_branch_status_empty_runs() {
        let s = derive_branch_status(&[]);
        assert_eq!(s.overall, "none");
        assert_eq!(s.total, 0);
    }

    // parse_checks_rollup

    #[test]
    fn parse_checks_rollup_none() {
        let s = parse_checks_rollup(None);
        assert_eq!(s.overall, "none");
    }

    #[test]
    fn parse_checks_rollup_empty_array() {
        let val = serde_json::json!([]);
        let s = parse_checks_rollup(Some(&val));
        assert_eq!(s.overall, "none");
    }

    #[test]
    fn parse_checks_rollup_all_success() {
        let val = serde_json::json!([
            {"conclusion": "SUCCESS"},
            {"conclusion": "NEUTRAL"},
            {"conclusion": "SKIPPED"}
        ]);
        let s = parse_checks_rollup(Some(&val));
        assert_eq!(s.overall, "success");
        assert_eq!(s.passing, 3);
    }

    #[test]
    fn parse_checks_rollup_with_failure() {
        let val = serde_json::json!([
            {"conclusion": "SUCCESS"},
            {"conclusion": "FAILURE"}
        ]);
        let s = parse_checks_rollup(Some(&val));
        assert_eq!(s.overall, "failure");
        assert_eq!(s.passing, 1);
        assert_eq!(s.failing, 1);
    }

    #[test]
    fn parse_checks_rollup_pending_from_state() {
        let val = serde_json::json!([
            {"state": "PENDING"}
        ]);
        let s = parse_checks_rollup(Some(&val));
        assert_eq!(s.overall, "pending");
        assert_eq!(s.pending, 1);
    }

    #[test]
    fn parse_checks_rollup_conclusion_over_state() {
        // When both conclusion and state are present, conclusion is checked first
        let val = serde_json::json!([
            {"conclusion": "SUCCESS", "state": "FAILURE"}
        ]);
        let s = parse_checks_rollup(Some(&val));
        assert_eq!(s.passing, 1);
    }

    // parse_pr_json

    #[test]
    fn parse_pr_json_full() {
        let val = serde_json::json!({
            "number": 42,
            "title": "Fix bug",
            "state": "OPEN",
            "url": "https://github.com/user/repo/pull/42",
            "isDraft": false,
            "headRefName": "fix-bug",
            "reviewDecision": "APPROVED",
            "statusCheckRollup": [{"conclusion": "SUCCESS"}],
            "mergeStateStatus": "CLEAN"
        });
        let pr = parse_pr_json(&val).unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.title, "Fix bug");
        assert_eq!(pr.state, "OPEN");
        assert_eq!(pr.head_ref_name, "fix-bug");
        assert_eq!(pr.review_decision, Some("APPROVED".to_string()));
        assert_eq!(pr.checks_status.passing, 1);
        assert_eq!(pr.merge_state_status, Some("CLEAN".to_string()));
        assert!(pr.actions.can_merge);
        assert!(!pr.actions.can_mark_ready);
        assert!(!pr.actions.can_update_branch);
    }

    #[test]
    fn parse_pr_json_minimal() {
        let val = serde_json::json!({
            "number": 1,
            "title": "",
            "state": "MERGED",
            "url": "",
            "isDraft": true,
            "headRefName": "main"
        });
        let pr = parse_pr_json(&val).unwrap();
        assert_eq!(pr.number, 1);
        assert!(pr.is_draft);
        assert_eq!(pr.review_decision, None);
        assert_eq!(pr.checks_status.overall, "none");
        assert_eq!(pr.merge_state_status, None);
        assert!(!pr.actions.can_merge);
        assert!(!pr.actions.can_mark_ready);
        assert!(!pr.actions.can_update_branch);
    }

    #[test]
    fn parse_pr_json_draft_open_can_mark_ready() {
        let val = serde_json::json!({
            "number": 7,
            "title": "WIP",
            "state": "OPEN",
            "url": "https://github.com/user/repo/pull/7",
            "isDraft": true,
            "headRefName": "wip-branch",
            "statusCheckRollup": [{"state": "PENDING"}],
            "mergeStateStatus": "BEHIND"
        });
        let pr = parse_pr_json(&val).unwrap();
        assert!(!pr.actions.can_merge);
        assert!(pr.actions.can_mark_ready);
        assert!(pr.actions.can_update_branch);
    }

    #[test]
    fn parse_pr_json_blocks_merge_when_checks_pending_or_failing() {
        let val = serde_json::json!({
            "number": 8,
            "title": "Feature",
            "state": "OPEN",
            "url": "https://github.com/user/repo/pull/8",
            "isDraft": false,
            "headRefName": "feature",
            "statusCheckRollup": [
                {"state": "PENDING"},
                {"conclusion": "FAILURE"}
            ],
            "mergeStateStatus": "CLEAN"
        });
        let pr = parse_pr_json(&val).unwrap();
        assert!(!pr.actions.can_merge);
        assert!(!pr.actions.can_mark_ready);
    }

    // group_runs_by_branch

    #[test]
    fn group_runs_by_branch_groups_and_dedupes() {
        let runs = vec![
            GitHubWorkflowRun {
                id: 1,
                name: "CI".to_string(),
                display_title: "old".to_string(),
                head_branch: "main".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                url: String::new(),
                event: "push".to_string(),
                created_at: "2025-01-01T00:00:00Z".to_string(),
                updated_at: String::new(),
            },
            GitHubWorkflowRun {
                id: 2,
                name: "CI".to_string(),
                display_title: "new".to_string(),
                head_branch: "main".to_string(),
                status: "completed".to_string(),
                conclusion: Some("failure".to_string()),
                url: String::new(),
                event: "push".to_string(),
                created_at: "2025-01-02T00:00:00Z".to_string(),
                updated_at: String::new(),
            },
            GitHubWorkflowRun {
                id: 3,
                name: "Lint".to_string(),
                display_title: "lint".to_string(),
                head_branch: "feature".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                url: String::new(),
                event: "push".to_string(),
                created_at: "2025-01-01T00:00:00Z".to_string(),
                updated_at: String::new(),
            },
        ];

        let grouped = group_runs_by_branch(runs);
        assert_eq!(grouped.len(), 2);

        // "main" should have only the latest CI run (id=2, failure)
        let main_branch = &grouped["main"];
        assert_eq!(main_branch.runs.len(), 1);
        assert_eq!(main_branch.runs[0].id, 2);
        assert_eq!(main_branch.status.overall, "failure");

        // "feature" should have one run
        let feature_branch = &grouped["feature"];
        assert_eq!(feature_branch.runs.len(), 1);
        assert_eq!(feature_branch.status.overall, "success");
    }

    #[test]
    fn group_runs_by_branch_empty() {
        let grouped = group_runs_by_branch(vec![]);
        assert!(grouped.is_empty());
    }
}
