//! Creating and removing a project's worktrees: the one path the desktop and
//! the server both take, so a worktree made from the phone lands where one made
//! on the desktop does.

use anyhow::Result;

use crate::types::{CreateWorktreeRequest, WorkbenchSettings};

/// Create a worktree of a registered project. What the request leaves unset
/// (layout, start point, fetch) comes from the host's settings.
pub fn create(req: CreateWorktreeRequest) -> Result<String> {
    create_with(
        req,
        &crate::config::load_workbench_settings()?,
        &crate::cwd::registered()?,
    )
}

fn create_with(
    req: CreateWorktreeRequest,
    settings: &WorkbenchSettings,
    registered: &[String],
) -> Result<String> {
    crate::cwd::resolve_in(&req.repo_path, None, registered)?;
    crate::git::create_worktree(&with_settings(req, settings))
}

fn with_settings(
    mut req: CreateWorktreeRequest,
    settings: &WorkbenchSettings,
) -> CreateWorktreeRequest {
    req.strategy
        .get_or_insert_with(|| settings.worktree_strategy.clone());
    if req.new_branch {
        if req.start_point.is_none() {
            req.start_point = start_point(settings);
        }
        req.fetch_before_create
            .get_or_insert(settings.worktree_fetch_before_create);
    }
    req
}

/// `None` lets git auto-detect `origin/<default branch>`, as does an empty custom branch.
fn start_point(settings: &WorkbenchSettings) -> Option<String> {
    match settings.worktree_start_point.as_str() {
        "current" => Some("current".to_string()),
        "custom" => Some(settings.worktree_custom_branch.trim())
            .filter(|b| !b.is_empty())
            .map(String::from),
        _ => None,
    }
}

/// Remove a known worktree of a registered project, then its branch when
/// `delete_branch` is set. A branch git won't delete (unmerged) stays, logged:
/// the worktree is already gone.
pub fn remove(
    repo_path: &str,
    worktree_path: &str,
    force: bool,
    delete_branch: bool,
) -> Result<()> {
    crate::cwd::resolve_cwd(repo_path, Some(worktree_path))?;
    let branch = delete_branch
        .then(|| crate::git::list_worktrees(repo_path).ok())
        .flatten()
        .and_then(|list| list.into_iter().find(|w| w.path == worktree_path))
        .map(|w| w.branch)
        .filter(|b| !b.is_empty());
    crate::git::remove_worktree(repo_path, worktree_path, force)?;
    if let Some(branch) = branch {
        if let Err(e) = crate::git::delete_branch(repo_path, &branch, false) {
            log::warn!("[worktrees] Failed to delete branch {branch}: {e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(new_branch: bool) -> CreateWorktreeRequest {
        CreateWorktreeRequest {
            repo_path: "/repo".into(),
            branch: "feature".into(),
            new_branch,
            path: None,
            copy_options: None,
            strategy: None,
            start_point: None,
            fetch_before_create: None,
        }
    }

    fn settings(strategy: &str, start: &str, custom: &str, fetch: bool) -> WorkbenchSettings {
        WorkbenchSettings {
            worktree_strategy: strategy.into(),
            worktree_start_point: start.into(),
            worktree_custom_branch: custom.into(),
            worktree_fetch_before_create: fetch,
            ..Default::default()
        }
    }

    #[test]
    fn unset_fields_come_from_the_settings() {
        let req = with_settings(
            request(true),
            &settings("inside", "custom", " develop ", false),
        );
        assert_eq!(req.strategy.as_deref(), Some("inside"));
        assert_eq!(req.start_point.as_deref(), Some("develop"));
        assert_eq!(req.fetch_before_create, Some(false));

        let req = with_settings(request(true), &settings("sibling", "current", "", true));
        assert_eq!(req.start_point.as_deref(), Some("current"));
        assert_eq!(req.fetch_before_create, Some(true));
    }

    #[test]
    fn auto_and_an_empty_custom_branch_leave_the_start_point_to_git() {
        let auto = with_settings(request(true), &settings("sibling", "auto", "x", true));
        assert_eq!(auto.start_point, None);
        let empty = with_settings(request(true), &settings("sibling", "custom", "  ", true));
        assert_eq!(empty.start_point, None);
    }

    #[test]
    fn an_explicit_field_wins_over_the_settings() {
        let mut req = request(true);
        req.strategy = Some("sibling".into());
        req.start_point = Some("release".into());
        req.fetch_before_create = Some(true);
        let req = with_settings(req, &settings("inside", "current", "", false));
        assert_eq!(req.strategy.as_deref(), Some("sibling"));
        assert_eq!(req.start_point.as_deref(), Some("release"));
        assert_eq!(req.fetch_before_create, Some(true));
    }

    #[test]
    fn an_existing_branch_takes_no_start_point_or_fetch() {
        let req = with_settings(request(false), &settings("inside", "current", "", false));
        assert_eq!(req.strategy.as_deref(), Some("inside"));
        assert_eq!(req.start_point, None);
        assert_eq!(req.fetch_before_create, None);
    }

    #[test]
    fn an_unregistered_repo_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut req = request(true);
        req.repo_path = dir.path().to_string_lossy().into();
        let err = create_with(req, &WorkbenchSettings::default(), &[]).unwrap_err();
        assert!(
            err.downcast_ref::<crate::cwd::NotAllowed>().is_some(),
            "{err}"
        );
    }
}
