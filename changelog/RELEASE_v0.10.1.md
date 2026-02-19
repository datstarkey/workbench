# Release v0.10.1

**Released:** 2026-02-19
**Previous version:** v0.10.0

A focused fix for worktree creation that prevents new branches from accidentally inheriting work-in-progress changes from your current feature branch.

## Bug Fixes

- **New worktree branches now start from the correct base** -- Previously, creating a worktree with a new branch would base it on whatever commit HEAD pointed to, which meant starting from a feature branch would carry over uncommitted or in-progress work. New branches now automatically fetch the latest remote state and branch from `origin/main` (or `origin/master`), ensuring a clean starting point every time.

## Improvements

- **Configurable worktree branch origin** -- Two new settings in Workbench Settings give you control over how new worktree branches are created. "Fetch before creating worktree" (on by default) ensures you always start from the latest remote state. "Branch new worktrees from" lets you choose between "Auto" (uses the remote's default branch) and "Current branch" if you prefer the previous behavior.
