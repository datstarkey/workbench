use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::github;
use crate::trello_automation;
use crate::types::{
    GitHubCheckTransitionEvent, GitHubProjectStatus, GitHubProjectStatusEvent, GitHubWorkflowRun,
    TrelloMergeActionAppliedEvent,
};

const FAST_POLL_INTERVAL: Duration = Duration::from_secs(30);
const SLOW_POLL_INTERVAL: Duration = Duration::from_secs(180);
const WORKER_TICK: Duration = Duration::from_millis(500);
const IN_FLIGHT_BACKOFF: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
struct PollProjectState {
    next_poll_at: Instant,
    persistent: bool,
}

struct PollerState {
    projects: HashMap<String, PollProjectState>,
    previous_check_buckets: HashMap<String, HashMap<String, String>>,
    previous_pr_states: HashMap<String, String>,
}

pub struct GitHubPoller {
    state: Arc<Mutex<PollerState>>,
    stop: Arc<AtomicBool>,
}

impl GitHubPoller {
    pub fn new(app_handle: AppHandle) -> Self {
        let poller = Self {
            state: Arc::new(Mutex::new(PollerState {
                projects: HashMap::new(),
                previous_check_buckets: HashMap::new(),
                previous_pr_states: HashMap::new(),
            })),
            stop: Arc::new(AtomicBool::new(false)),
        };
        poller.start_worker(app_handle);
        poller
    }

    pub fn set_tracked_projects(&self, project_paths: Vec<String>) {
        let tracked: HashSet<String> = project_paths
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();

        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());

        state
            .projects
            .retain(|path, project| !project.persistent || tracked.contains(path));

        for path in tracked {
            let entry = state.projects.entry(path).or_insert(PollProjectState {
                next_poll_at: now,
                persistent: true,
            });
            entry.persistent = true;
            entry.next_poll_at = now;
        }

        let active_prefixes: Vec<String> = state
            .projects
            .keys()
            .map(|path| format!("{path}::"))
            .collect();
        state
            .previous_check_buckets
            .retain(|key, _| active_prefixes.iter().any(|prefix| key.starts_with(prefix)));
        state
            .previous_pr_states
            .retain(|key, _| active_prefixes.iter().any(|prefix| key.starts_with(prefix)));
    }

    /// Push a project's next poll time to `now + SLOW_POLL_INTERVAL`.
    /// Call after an explicit refresh so the poller doesn't double-fetch.
    pub fn defer_project(&self, project_path: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(project) = state.projects.get_mut(project_path) {
            project.next_poll_at = Instant::now() + SLOW_POLL_INTERVAL;
        }
    }

    fn start_worker(&self, app_handle: AppHandle) {
        let state = Arc::clone(&self.state);
        let stop = Arc::clone(&self.stop);

        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let due_projects = Self::take_due_projects(&state);

                if !due_projects.is_empty() {
                    // Fetch statuses for all due projects in parallel
                    let results: Vec<_> = std::thread::scope(|s| {
                        let handles: Vec<_> = due_projects
                            .iter()
                            .map(|(project_path, _)| {
                                let path = project_path.as_str();
                                s.spawn(move || github::get_project_status(path))
                            })
                            .collect();
                        handles
                            .into_iter()
                            .map(|h| {
                                h.join().unwrap_or_else(|_| GitHubProjectStatus {
                                    remote: None,
                                    prs: vec![],
                                    branch_runs: HashMap::new(),
                                    pr_checks: HashMap::new(),
                                })
                            })
                            .collect()
                    });

                    // Process results sequentially (transition detection, event emission, state updates)
                    for ((project_path, persistent), status) in
                        due_projects.into_iter().zip(results)
                    {
                        let interval = if status_has_pending(&status) {
                            FAST_POLL_INTERVAL
                        } else {
                            SLOW_POLL_INTERVAL
                        };
                        let transitions = detect_check_transitions_and_update(
                            &state,
                            project_path.as_str(),
                            &status,
                        );
                        let merged_branches = detect_merged_pr_transitions_and_update(
                            &state,
                            project_path.as_str(),
                            &status,
                        );

                        let _ = app_handle.emit(
                            "github:project-status",
                            GitHubProjectStatusEvent {
                                project_path: project_path.clone(),
                                status,
                            },
                        );
                        for transition in transitions {
                            let _ = app_handle.emit("github:check-transition", transition);
                        }
                        for merge_event in apply_trello_merge_actions(
                            &project_path,
                            merged_branches,
                            trello_automation::apply_merge_action_for_branch,
                        ) {
                            let _ = app_handle.emit("trello:merge-action-applied", merge_event);
                        }

                        let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
                        if persistent {
                            if let Some(project) = guard.projects.get_mut(&project_path) {
                                project.next_poll_at = Instant::now() + interval;
                            }
                        } else {
                            guard.projects.remove(&project_path);
                        }
                    }
                }

                std::thread::sleep(WORKER_TICK);
            }
        });
    }

    fn take_due_projects(state: &Arc<Mutex<PollerState>>) -> Vec<(String, bool)> {
        let now = Instant::now();
        let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
        let mut due = Vec::new();

        for (project_path, project) in &mut guard.projects {
            if project.next_poll_at <= now {
                due.push((project_path.clone(), project.persistent));
                project.next_poll_at = now + IN_FLIGHT_BACKOFF;
            }
        }

        due
    }
}

impl Drop for GitHubPoller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Fast-poll only while a branch we actually surface is mid-run.
///
/// Scoped to branches with an open PR (plus the checked-out branches the sidebar
/// shows) rather than every branch in `gh run list` — otherwise one stale queued
/// run on a long-abandoned branch pins the project to the fast interval forever.
fn status_has_pending(status: &GitHubProjectStatus) -> bool {
    let open_pr_branches: HashSet<&str> = status
        .prs
        .iter()
        .filter(|pr| pr.state == "OPEN")
        .map(|pr| pr.head_ref_name.as_str())
        .collect();

    status
        .branch_runs
        .iter()
        .filter(|(branch, _)| open_pr_branches.contains(branch.as_str()))
        .any(|(_, runs)| runs.status.pending > 0)
}

fn detect_merged_pr_transitions_and_update(
    state: &Arc<Mutex<PollerState>>,
    project_path: &str,
    status: &GitHubProjectStatus,
) -> Vec<String> {
    let mut merged_branches = Vec::new();
    let prefix = format!("{project_path}::");
    let mut seen_keys = HashSet::new();
    let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());

    for pr in &status.prs {
        let key = pr_key(project_path, pr.number);
        seen_keys.insert(key.clone());
        let previous = guard.previous_pr_states.insert(key, pr.state.clone());
        if matches!(previous.as_deref(), Some(state) if state != "MERGED") && pr.state == "MERGED" {
            merged_branches.push(pr.head_ref_name.clone());
        }
    }

    guard
        .previous_pr_states
        .retain(|key, _| !key.starts_with(&prefix) || seen_keys.contains(key));

    merged_branches
}

fn apply_trello_merge_actions<F, E>(
    project_path: &str,
    merged_branches: Vec<String>,
    mut apply_action: F,
) -> Vec<TrelloMergeActionAppliedEvent>
where
    F: FnMut(&str, &str) -> Result<Option<String>, E>,
    E: std::fmt::Display,
{
    let mut events = Vec::new();
    for branch in merged_branches {
        match apply_action(project_path, &branch) {
            Ok(Some(card_id)) => events.push(TrelloMergeActionAppliedEvent {
                project_path: project_path.to_string(),
                branch,
                card_id,
            }),
            Ok(None) => {}
            Err(err) => {
                log::warn!(
                    "[GitHubPoller] Failed Trello merge action for {} ({}): {}",
                    project_path,
                    branch,
                    err
                );
            }
        }
    }
    events
}

/// Detect pending → pass/fail transitions so the frontend can toast them.
///
/// Driven by workflow runs (`gh run list`, REST) rather than per-PR check detail,
/// which is a GraphQL call and no longer part of the poll. Scoped to branches with
/// an open PR to keep notification volume the same as before.
fn detect_check_transitions_and_update(
    state: &Arc<Mutex<PollerState>>,
    project_path: &str,
    status: &GitHubProjectStatus,
) -> Vec<GitHubCheckTransitionEvent> {
    let mut transitions = Vec::new();
    let prefix = format!("{project_path}::");
    let mut seen_keys = HashSet::new();
    let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());

    for pr in status.prs.iter().filter(|pr| pr.state == "OPEN") {
        let Some(branch) = status.branch_runs.get(&pr.head_ref_name) else {
            continue;
        };
        let key = pr_key(project_path, pr.number);
        seen_keys.insert(key.clone());

        if let Some(old_buckets) = guard.previous_check_buckets.get(&key) {
            transitions.extend(run_transitions_for_pr(
                old_buckets,
                &branch.runs,
                project_path,
                pr.number,
            ));
        }

        guard
            .previous_check_buckets
            .insert(key, build_bucket_map(&branch.runs));
    }

    guard
        .previous_check_buckets
        .retain(|key, _| !key.starts_with(&prefix) || seen_keys.contains(key));

    transitions
}

/// Bucket a workflow run using the same vocabulary `gh pr checks` emits, so the
/// frontend's bucket handling is unchanged.
fn run_bucket(run: &GitHubWorkflowRun) -> &'static str {
    if run.status != "completed" {
        return "pending";
    }
    match run.conclusion.as_deref() {
        Some("success") | Some("skipped") | Some("neutral") => "pass",
        Some("cancelled") => "cancel",
        Some(_) => "fail",
        None => "pending",
    }
}

fn run_transitions_for_pr(
    old_buckets: &HashMap<String, String>,
    runs: &[GitHubWorkflowRun],
    project_path: &str,
    pr_number: u64,
) -> Vec<GitHubCheckTransitionEvent> {
    let mut transitions = Vec::new();

    for run in runs {
        let bucket = run_bucket(run);
        let Some(previous) = old_buckets.get(&run.name) else {
            continue;
        };
        if previous == "pending" && (bucket == "pass" || bucket == "fail") {
            transitions.push(GitHubCheckTransitionEvent {
                project_path: project_path.to_string(),
                pr_number,
                name: run.name.clone(),
                bucket: bucket.to_string(),
            });
        }
    }

    transitions
}

/// `group_runs_by_branch` keeps only the latest run per workflow name, so the
/// workflow name alone is a stable per-branch key.
fn build_bucket_map(runs: &[GitHubWorkflowRun]) -> HashMap<String, String> {
    runs.iter()
        .map(|run| (run.name.clone(), run_bucket(run).to_string()))
        .collect()
}

fn pr_key(project_path: &str, pr_number: u64) -> String {
    format!("{project_path}::{pr_number}")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::{
        apply_trello_merge_actions, build_bucket_map, detect_merged_pr_transitions_and_update,
        run_transitions_for_pr, status_has_pending, PollProjectState, PollerState,
    };
    use crate::types::{
        GitHubBranchRuns, GitHubChecksStatus, GitHubPR, GitHubPRActions, GitHubProjectStatus,
        GitHubWorkflowRun,
    };

    fn make_run(name: &str, status: &str, conclusion: Option<&str>) -> GitHubWorkflowRun {
        GitHubWorkflowRun {
            id: 1,
            name: name.to_string(),
            display_title: format!("{name} run"),
            head_branch: "feature/x".to_string(),
            status: status.to_string(),
            conclusion: conclusion.map(String::from),
            url: String::new(),
            event: "push".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn checks_status(pending: u32) -> GitHubChecksStatus {
        GitHubChecksStatus {
            overall: if pending > 0 { "pending" } else { "success" }.to_string(),
            total: pending.max(1),
            passing: if pending > 0 { 0 } else { 1 },
            failing: 0,
            pending,
        }
    }

    fn make_pr(number: u64, branch: &str, state: &str) -> GitHubPR {
        GitHubPR {
            number,
            title: format!("PR {number}"),
            state: state.to_string(),
            url: String::new(),
            is_draft: false,
            head_ref_name: branch.to_string(),
            review_decision: None,
            checks_status: checks_status(0),
            merge_state_status: Some("CLEAN".to_string()),
            actions: GitHubPRActions {
                can_merge: true,
                can_mark_ready: false,
                can_update_branch: false,
            },
        }
    }

    #[test]
    fn status_has_pending_true_for_branch_with_open_pr() {
        let mut branch_runs = HashMap::new();
        branch_runs.insert(
            "feature/x".to_string(),
            GitHubBranchRuns {
                status: checks_status(1),
                runs: vec![],
            },
        );
        let status = GitHubProjectStatus {
            remote: None,
            prs: vec![make_pr(42, "feature/x", "OPEN")],
            branch_runs,
            pr_checks: HashMap::new(),
        };

        assert!(status_has_pending(&status));
    }

    #[test]
    fn status_has_pending_ignores_branches_without_an_open_pr() {
        // A stale queued run on an abandoned branch must not pin the project to the
        // fast poll interval forever.
        let mut branch_runs = HashMap::new();
        branch_runs.insert(
            "abandoned".to_string(),
            GitHubBranchRuns {
                status: checks_status(1),
                runs: vec![],
            },
        );
        let status = GitHubProjectStatus {
            remote: None,
            prs: vec![make_pr(42, "feature/x", "OPEN")],
            branch_runs,
            pr_checks: HashMap::new(),
        };

        assert!(!status_has_pending(&status));
    }

    #[test]
    fn status_has_pending_ignores_merged_pr_branches() {
        let mut branch_runs = HashMap::new();
        branch_runs.insert(
            "feature/x".to_string(),
            GitHubBranchRuns {
                status: checks_status(1),
                runs: vec![],
            },
        );
        let status = GitHubProjectStatus {
            remote: None,
            prs: vec![make_pr(42, "feature/x", "MERGED")],
            branch_runs,
            pr_checks: HashMap::new(),
        };

        assert!(!status_has_pending(&status));
    }

    #[test]
    fn status_has_pending_false_without_pending_states() {
        let status = GitHubProjectStatus {
            remote: None,
            prs: vec![],
            branch_runs: HashMap::new(),
            pr_checks: HashMap::new(),
        };

        assert!(!status_has_pending(&status));
    }

    #[test]
    fn run_transition_detects_pending_to_pass() {
        let old = HashMap::from([(String::from("CI"), String::from("pending"))]);
        let runs = vec![make_run("CI", "completed", Some("success"))];

        let transitions = run_transitions_for_pr(&old, &runs, "/repo", 1);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].project_path, "/repo");
        assert_eq!(transitions[0].pr_number, 1);
        assert_eq!(transitions[0].name, "CI");
        assert_eq!(transitions[0].bucket, "pass");
    }

    #[test]
    fn run_transition_detects_pending_to_fail() {
        let old = HashMap::from([(String::from("CI"), String::from("pending"))]);
        let runs = vec![make_run("CI", "completed", Some("failure"))];

        let transitions = run_transitions_for_pr(&old, &runs, "/repo", 1);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].bucket, "fail");
    }

    #[test]
    fn run_transition_ignores_non_pending_previous_state() {
        let old = HashMap::from([(String::from("CI"), String::from("pass"))]);
        let runs = vec![make_run("CI", "completed", Some("failure"))];

        let transitions = run_transitions_for_pr(&old, &runs, "/repo", 1);
        assert!(transitions.is_empty());
    }

    #[test]
    fn run_transition_ignores_still_running_workflows() {
        let old = HashMap::from([(String::from("CI"), String::from("pending"))]);
        let runs = vec![make_run("CI", "in_progress", None)];

        let transitions = run_transitions_for_pr(&old, &runs, "/repo", 1);
        assert!(transitions.is_empty());
    }

    #[test]
    fn run_transition_ignores_cancelled_workflows() {
        // `cancel` is neither pass nor fail — cancelling a run shouldn't toast.
        let old = HashMap::from([(String::from("CI"), String::from("pending"))]);
        let runs = vec![make_run("CI", "completed", Some("cancelled"))];

        let transitions = run_transitions_for_pr(&old, &runs, "/repo", 1);
        assert!(transitions.is_empty());
    }

    #[test]
    fn merged_pr_transition_detects_open_to_merged_once() {
        let state = Arc::new(Mutex::new(PollerState {
            projects: HashMap::<String, PollProjectState>::new(),
            previous_check_buckets: HashMap::new(),
            previous_pr_states: HashMap::new(),
        }));
        let project_path = "/repo";

        let open_status = GitHubProjectStatus {
            remote: None,
            prs: vec![make_pr(42, "feature/x", "OPEN")],
            branch_runs: HashMap::new(),
            pr_checks: HashMap::new(),
        };
        let merged_status = GitHubProjectStatus {
            remote: None,
            prs: vec![make_pr(42, "feature/x", "MERGED")],
            branch_runs: HashMap::new(),
            pr_checks: HashMap::new(),
        };

        let first = detect_merged_pr_transitions_and_update(&state, project_path, &open_status);
        assert!(first.is_empty());

        let second = detect_merged_pr_transitions_and_update(&state, project_path, &merged_status);
        assert_eq!(second, vec!["feature/x".to_string()]);

        let third = detect_merged_pr_transitions_and_update(&state, project_path, &merged_status);
        assert!(third.is_empty());
    }

    #[test]
    fn apply_trello_merge_actions_emits_only_successful_events() {
        let events = apply_trello_merge_actions(
            "/repo",
            vec![
                "feature/a".to_string(),
                "feature/b".to_string(),
                "feature/c".to_string(),
            ],
            |_project_path, branch| -> Result<Option<String>, String> {
                match branch {
                    "feature/a" => Ok(Some("card-1".to_string())),
                    "feature/b" => Ok(None),
                    _ => Err("boom".to_string()),
                }
            },
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].project_path, "/repo");
        assert_eq!(events[0].branch, "feature/a");
        assert_eq!(events[0].card_id, "card-1");
    }

    #[test]
    fn apply_trello_merge_actions_handles_empty_branches() {
        let events = apply_trello_merge_actions(
            "/repo",
            vec![],
            |_project_path, _branch| -> Result<Option<String>, String> { Ok(None) },
        );

        assert!(events.is_empty());
    }

    #[test]
    fn build_bucket_map_keys_by_workflow_name() {
        let runs = vec![
            make_run("lint", "in_progress", None),
            make_run("build", "completed", Some("success")),
        ];
        let map = build_bucket_map(&runs);
        assert_eq!(map.get("lint"), Some(&"pending".to_string()));
        assert_eq!(map.get("build"), Some(&"pass".to_string()));
    }
}
