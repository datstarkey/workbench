# Release v0.11.0

**Released:** 2026-02-23
**Previous version:** v0.10.1

This release adds the ability to specify a custom branch as the starting point for new worktrees, and fixes an issue where the "auto" start point setting was not being applied correctly.

## New Features

- **Custom branch start point for worktrees** -- You can now choose a specific branch or ref as the base for new worktree branches. In Workbench Settings under "Branch new worktrees from", select "Custom branch" and enter any branch name (e.g. `develop`, `staging`, or `origin/release-2.0`). This is useful when your team's workflow branches from something other than the repository's default branch.

## Bug Fixes

- **Fixed "Auto" worktree start point not working correctly** -- The "Auto (origin default branch)" setting for worktree creation was being ignored in some cases, causing new branches to start from an unexpected ref instead of the remote's default branch. This has been corrected so "Auto" reliably detects and uses `origin/main` or `origin/master` as intended.
