use std::collections::HashMap;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::types::{
    GitHubBranchRuns, GitHubCheckDetail, GitHubChecksStatus, GitHubPR, GitHubProjectStatus,
    GitHubRemote, GitHubWorkflowRun,
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
    let (owner, repo) = if let Some(rest) = url.strip_prefix("git@github.com:") {
        let rest = rest.trim_end_matches(".git");
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() != 2 {
            bail!("Cannot parse SSH remote: {url}");
        }
        (parts[0].to_string(), parts[1].to_string())
    } else if url.contains("github.com/") {
        let after = url
            .split("github.com/")
            .nth(1)
            .context("Cannot parse HTTPS remote")?;
        let after = after.trim_end_matches(".git");
        let parts: Vec<&str> = after.splitn(2, '/').collect();
        if parts.len() != 2 {
            bail!("Cannot parse HTTPS remote: {url}");
        }
        (parts[0].to_string(), parts[1].to_string())
    } else {
        bail!("Not a GitHub remote: {url}");
    };

    let html_url = format!("https://github.com/{owner}/{repo}");
    Ok(GitHubRemote {
        owner,
        repo,
        html_url,
    })
}

pub fn list_project_prs(path: &str) -> Result<Vec<GitHubPR>> {
    let fields = "number,title,state,url,isDraft,headRefName,reviewDecision,statusCheckRollup,mergeStateStatus";
    let result = gh_output(
        &["pr", "list", "--state", "all", "--limit", "100", "--json", fields],
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
    let result = gh_output(
        &["run", "list", "--limit", "200", "--json", fields],
        path,
    );

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
        let mut seen_workflows: std::collections::HashSet<String> = std::collections::HashSet::new();
        let deduped: Vec<GitHubWorkflowRun> = branch_runs
            .into_iter()
            .filter(|r| seen_workflows.insert(r.name.clone()))
            .collect();

        let status = derive_branch_status(&deduped);
        result.insert(branch, GitHubBranchRuns {
            status,
            runs: deduped,
        });
    }

    result
}

fn derive_branch_status(runs: &[GitHubWorkflowRun]) -> GitHubChecksStatus {
    if runs.is_empty() {
        return GitHubChecksStatus {
            overall: "none".to_string(),
            total: 0,
            passing: 0,
            failing: 0,
            pending: 0,
        };
    }

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
            // queued, in_progress, waiting
            pending += 1;
        }
    }

    let total = passing + failing + pending;
    let overall = if failing > 0 {
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

    Ok(GitHubPR {
        number: v["number"].as_u64().unwrap_or(0),
        title: v["title"].as_str().unwrap_or("").to_string(),
        state: v["state"].as_str().unwrap_or("OPEN").to_string(),
        url: v["url"].as_str().unwrap_or("").to_string(),
        is_draft: v["isDraft"].as_bool().unwrap_or(false),
        head_ref_name: v["headRefName"].as_str().unwrap_or("").to_string(),
        review_decision: v["reviewDecision"].as_str().map(String::from),
        checks_status: checks,
        merge_state_status: v["mergeStateStatus"].as_str().map(String::from),
    })
}

fn parse_checks_rollup(rollup: Option<&serde_json::Value>) -> GitHubChecksStatus {
    let empty = GitHubChecksStatus {
        overall: "none".to_string(),
        total: 0,
        passing: 0,
        failing: 0,
        pending: 0,
    };

    let arr = match rollup.and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => a,
        _ => return empty,
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

    let total = passing + failing + pending;
    let overall = if failing > 0 {
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

pub fn list_pr_checks(path: &str, pr_number: u64) -> Result<Vec<GitHubCheckDetail>> {
    let fields = "name,bucket,completedAt,startedAt,link,workflow,description";
    let result = gh_output(
        &[
            "pr",
            "checks",
            &pr_number.to_string(),
            "--json",
            fields,
        ],
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
            &format!("repos/{}/{}/pulls/{}/update-branch", remote.owner, remote.repo, pr_number),
            "-X",
            "PUT",
        ],
        path,
    )?;
    Ok(())
}

pub fn rerun_workflow(path: &str, run_id: u64) -> Result<()> {
    gh_output(&["run", "rerun", &run_id.to_string()], path)?;
    Ok(())
}

pub fn mark_pr_ready(path: &str, pr_number: u64) -> Result<()> {
    gh_output(&["pr", "ready", &pr_number.to_string()], path)?;
    Ok(())
}

pub fn merge_pr(path: &str, pr_number: u64) -> Result<()> {
    gh_output(
        &["pr", "merge", &pr_number.to_string(), "--squash"],
        path,
    )?;
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
        Command::new("cmd")
            .args(["/c", "start", url])
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
    fn parse_non_github_remote_fails() {
        assert!(parse_github_remote("https://gitlab.com/user/repo.git").is_err());
    }
}
