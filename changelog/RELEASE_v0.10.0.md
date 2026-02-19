# Release v0.10.0

**Released:** 2026-02-19
**Previous version:** v0.9.0

This release introduces a built-in Git sidebar, giving you a full source control workflow without leaving Workbench. You can view file status, stage and unstage changes, commit, manage branches and stashes, and sync with remotes -- all from a dedicated panel alongside your terminals. GitHub PR actions are also now more responsive, updating the UI immediately after you merge, update, or mark a pull request as ready.

## New Features

- **Git sidebar** -- A new "Git" tab in the right sidebar provides a complete source control interface for the active workspace. It includes collapsible sections for staged and unstaged files with one-click staging, a commit message input with keyboard shortcut support, a branch list with checkout, a scrollable commit log, and full stash management (save, pop, apply, drop). The sidebar automatically refreshes when Claude or Codex modifies files via the existing hook bridge, so the status always reflects the latest changes on disk.

- **Remote sync controls** -- The git sidebar header displays your current branch with ahead/behind indicators relative to the remote. Fetch, pull, and push buttons appear contextually: pull shows when you're behind the remote, push shows when you have local commits or need to publish a new branch. If there's no upstream set, push automatically creates one.

- **Discard changes** -- Individual unstaged files can be discarded directly from the sidebar, reverting them to their last committed state without needing to open a terminal.

## Bug Fixes

- **GitHub PR status now updates instantly after actions** -- Previously, merging a pull request, updating its branch, or marking it as ready for review would leave the sidebar showing stale status until the next polling cycle (up to 90 seconds). These actions now fetch and display fresh PR status immediately, so you see the result of your action right away.
