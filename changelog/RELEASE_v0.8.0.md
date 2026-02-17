# Release v0.8.0

**Released:** 2026-02-17
**Previous version:** v0.7.0

This release brings a major upgrade to the GitHub sidebar with new actions for managing pull requests and workflows directly from Workbench, along with several bug fixes that improve branch tracking, session switching, and sidebar scrolling.

## New Features

- **Manage pull requests and CI workflows from the GitHub sidebar** -- The GitHub sidebar now lets you take action on your pull requests and CI runs without leaving Workbench. You can update a PR branch when it falls behind the base branch, re-run failed CI workflows with a single click, and convert draft pull requests to "Ready for Review" -- all from the sidebar header. A new "Other Open PRs" section lists all open pull requests for the project so you can quickly navigate between them. Failing CI checks now display their failure descriptions inline, so you can see what went wrong at a glance. Additionally, Workbench now shows toast notifications when CI checks complete, keeping you informed of status changes even when you are focused elsewhere.

## Bug Fixes

- **Fixed branch not updating in GitHub and Trello sidebars for main workspaces** -- When you switched branches in a main (non-worktree) workspace, the GitHub sidebar and Trello board panel continued to show data for the previous branch. Branch resolution now derives reactively from the git store, so the sidebar always reflects the current branch.

- **Fixed session startup command not syncing on session change** -- When switching between Claude or Codex sessions within a terminal pane, the startup command for that pane was not updated to match the new session. This could cause the wrong session to be resumed after a terminal restart. The startup command now syncs correctly whenever the active session changes. Additionally, the "awaiting input" indicator now clears properly when new output is received, preventing stale attention signals.

- **Fixed boards sidebar not scrolling** -- The right sidebar content area was missing an overflow rule, which prevented the Boards tab from scrolling when its content exceeded the visible area. Both the GitHub and Boards tabs now scroll correctly.
