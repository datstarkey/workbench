use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::github;
use crate::trello_automation;
use crate::types::{
    GitHubCheckDetail, GitHubCheckTransitionEvent, GitHubProjectStatus, GitHubProjectStatusEvent,
    TrelloMergeActionAppliedEvent,
};

const FAST_POLL_INTERVAL: Duration = Duration::from_secs(15);
const SLOW_POLL_INTERVAL: Duration = Duration::from_secs(90);
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

        let active_prefixes: Vec<String> = state.projects.keys().map(|path| format!("{path}::")).collect();
        state
            .previous_check_buckets
            .retain(|key, _| active_prefixes.iter().any(|prefix| key.starts_with(prefix)));
        state
            .previous_pr_states
            .retain(|key, _| active_prefixes.iter().any(|prefix| key.starts_with(prefix)));
    }

    pub fn request_refresh(&self, project_path: String) {
        let project_path = project_path.trim().to_string();
        if project_path.is_empty() {
            return;
        }

        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let entry = state.projects.entry(project_path).or_insert(PollProjectState {
            next_poll_at: now,
            persistent: false,
        });
        entry.next_poll_at = now;
    }

    fn start_worker(&self, app_handle: AppHandle) {
        let state = Arc::clone(&self.state);
        let stop = Arc::clone(&self.stop);

        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let due_projects = Self::take_due_projects(&state);

                for (project_path, persistent) in due_projects {
                    let status = github::get_project_status(&project_path);
                    let interval = if status_has_pending(&status) {
                        FAST_POLL_INTERVAL
                    } else {
                        SLOW_POLL_INTERVAL
                    };
                    let transitions =
                        detect_check_transitions_and_update(&state, project_path.as_str(), &status);
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
                    for branch in merged_branches {
                        match trello_automation::apply_merge_action_for_branch(&project_path, &branch) {
                            Ok(Some(card_id)) => {
                                let _ = app_handle.emit(
                                    "trello:merge-action-applied",
                                    TrelloMergeActionAppliedEvent {
                                        project_path: project_path.clone(),
                                        branch,
                                        card_id,
                                    },
                                );
                            }
                            Ok(None) => {}
                            Err(err) => {
                                eprintln!(
                                    "[GitHubPoller] Failed Trello merge action for {} ({}): {}",
                                    project_path, branch, err
                                );
                            }
                        }
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

fn status_has_pending(status: &GitHubProjectStatus) -> bool {
    if status
        .branch_runs
        .values()
        .any(|branch| branch.status.pending > 0)
    {
        return true;
    }

    status
        .pr_checks
        .values()
        .flatten()
        .any(|check| check.bucket == "pending")
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
        if matches!(previous.as_deref(), Some(state) if state != "MERGED") && pr.state == "MERGED"
        {
            merged_branches.push(pr.head_ref_name.clone());
        }
    }

    guard
        .previous_pr_states
        .retain(|key, _| !key.starts_with(&prefix) || seen_keys.contains(key));

    merged_branches
}

fn detect_check_transitions_and_update(
    state: &Arc<Mutex<PollerState>>,
    project_path: &str,
    status: &GitHubProjectStatus,
) -> Vec<GitHubCheckTransitionEvent> {
    let mut transitions = Vec::new();
    let prefix = format!("{project_path}::");
    let mut seen_keys = HashSet::new();
    let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());

    for (pr_number, checks) in &status.pr_checks {
        let key = pr_key(project_path, *pr_number);
        seen_keys.insert(key.clone());

        if let Some(old_buckets) = guard.previous_check_buckets.get(&key) {
            transitions.extend(check_transitions_for_pr(
                old_buckets,
                checks,
                project_path,
                *pr_number,
            ));
        }

        guard
            .previous_check_buckets
            .insert(key, build_bucket_map(checks));
    }

    guard
        .previous_check_buckets
        .retain(|key, _| !key.starts_with(&prefix) || seen_keys.contains(key));

    transitions
}

fn check_transitions_for_pr(
    old_buckets: &HashMap<String, String>,
    checks: &[GitHubCheckDetail],
    project_path: &str,
    pr_number: u64,
) -> Vec<GitHubCheckTransitionEvent> {
    let mut transitions = Vec::new();

    for check in checks {
        let key = format!("{}::{}", check.name, check.workflow);
        let Some(previous) = old_buckets.get(&key) else {
            continue;
        };
        if previous == "pending" && (check.bucket == "pass" || check.bucket == "fail") {
            transitions.push(GitHubCheckTransitionEvent {
                project_path: project_path.to_string(),
                pr_number,
                name: check.name.clone(),
                bucket: check.bucket.clone(),
            });
        }
    }

    transitions
}

fn build_bucket_map(checks: &[GitHubCheckDetail]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for check in checks {
        map.insert(
            format!("{}::{}", check.name, check.workflow),
            check.bucket.clone(),
        );
    }
    map
}

fn pr_key(project_path: &str, pr_number: u64) -> String {
    format!("{project_path}::{pr_number}")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::{
        build_bucket_map, check_transitions_for_pr, detect_merged_pr_transitions_and_update,
        status_has_pending, PollProjectState, PollerState,
    };
    use crate::types::{
        GitHubBranchRuns, GitHubCheckDetail, GitHubChecksStatus, GitHubPRActions, GitHubPR,
        GitHubProjectStatus,
    };

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
    fn status_has_pending_true_for_branch_runs() {
        let mut branch_runs = HashMap::new();
        branch_runs.insert(
            "main".to_string(),
            GitHubBranchRuns {
                status: checks_status(1),
                runs: vec![],
            },
        );
        let status = GitHubProjectStatus {
            remote: None,
            prs: vec![],
            branch_runs,
            pr_checks: HashMap::new(),
        };

        assert!(status_has_pending(&status));
    }

    #[test]
    fn status_has_pending_true_for_pr_checks() {
        let mut pr_checks = HashMap::new();
        pr_checks.insert(
            42,
            vec![GitHubCheckDetail {
                name: "CI".to_string(),
                bucket: "pending".to_string(),
                workflow: "build".to_string(),
                link: "".to_string(),
                started_at: None,
                completed_at: None,
                description: "".to_string(),
            }],
        );

        let status = GitHubProjectStatus {
            remote: None,
            prs: vec![],
            branch_runs: HashMap::new(),
            pr_checks,
        };

        assert!(status_has_pending(&status));
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
    fn check_transition_detects_pending_to_pass() {
        let old = HashMap::from([(String::from("CI::build"), String::from("pending"))]);
        let checks = vec![GitHubCheckDetail {
            name: "CI".to_string(),
            bucket: "pass".to_string(),
            workflow: "build".to_string(),
            link: "".to_string(),
            started_at: None,
            completed_at: None,
            description: "".to_string(),
        }];

        let transitions = check_transitions_for_pr(&old, &checks, "/repo", 1);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].project_path, "/repo");
        assert_eq!(transitions[0].pr_number, 1);
        assert_eq!(transitions[0].name, "CI");
        assert_eq!(transitions[0].bucket, "pass");
    }

    #[test]
    fn check_transition_ignores_non_pending_previous_state() {
        let old = HashMap::from([(String::from("CI::build"), String::from("pass"))]);
        let checks = vec![GitHubCheckDetail {
            name: "CI".to_string(),
            bucket: "fail".to_string(),
            workflow: "build".to_string(),
            link: "".to_string(),
            started_at: None,
            completed_at: None,
            description: "".to_string(),
        }];

        let transitions = check_transitions_for_pr(&old, &checks, "/repo", 1);
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
    fn build_bucket_map_uses_name_and_workflow_key() {
        let checks = vec![GitHubCheckDetail {
            name: "lint".to_string(),
            bucket: "pending".to_string(),
            workflow: "ci".to_string(),
            link: "".to_string(),
            started_at: None,
            completed_at: None,
            description: "".to_string(),
        }];
        let map = build_bucket_map(&checks);
        assert_eq!(map.get("lint::ci"), Some(&"pending".to_string()));
    }
}
