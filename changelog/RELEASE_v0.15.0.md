# Release v0.15.0

**Released:** 2026-03-04
**Previous version:** v0.14.1

This release improves workspace awareness with at-a-glance attention indicators on workspace tabs and ensures git and GitHub data stays fresh as you switch between projects.

## New Features

- **Workspace tabs now show attention state with color-coded indicators** -- The workspace pills in the top bar now reflect the session attention state of each workspace, matching the existing sidebar indicators. Tabs turn red when a session is awaiting your input, amber when Claude needs attention, and sky blue when Codex needs attention. This makes it much easier to spot which workspaces need you without switching to each one.

- **Git and GitHub data refreshes automatically when switching workspaces** -- Previously, switching between workspaces could show stale branch info, PR status, or CI results until the next polling cycle. Now, git state and GitHub status are refreshed immediately (with a 2-second throttle) whenever you switch to a workspace in a different project, so you always see current data.

## Bug Fixes

- **Fixed worktree delete dialog showing the wrong branch name** -- When deleting multiple worktrees in sequence, the confirmation dialog could display a stale branch name from a previous deletion request, potentially from a different project entirely. The dialog now correctly updates to show the branch being deleted.
