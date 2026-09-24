# Release v0.30.0

**Released:** 2026-09-24
**Previous version:** v0.29.1

A sidebar release. The right sidebar's Git and GitHub tabs are redesigned so it's always clear which branch you're on, what will happen when you click, and what is blocking a merge. A new Scripts tab runs your `package.json` scripts, and the project sidebar now shows each project as a tree of branches and worktrees.

## New Features

- **Redesigned Git tab.** (#115)
  - The branch name opens a searchable **branch picker**. It lists local and remote branches, can create a branch or a new worktree, and works from the keyboard.
  - One **sync button** says what it will do: Publish, Push 2, or Pull 3. A branch that has diverged from the remote is flagged instead of offering a pull that git would refuse.
  - The **commit box** is always visible when you have changes, and its button names what it will commit ("Commit 3 staged files"). A menu next to it holds Amend, Commit & push, and Stash all changes. ⌘↵ commits; ⌘⇧↵ commits and pushes.
  - **History** marks commits that haven't been pushed yet. Each commit has a menu to copy its SHA, open it on GitHub, or revert it.
  - The tab shows how many files have changed.
- **Redesigned GitHub tab.** (#115)
  - A **branch bar** shows which branch you're looking at, with a one-click way back to your current workspace.
  - A **merge status box** lists checks, review, and whether the branch is behind or conflicting. Below it is a single action button: Squash and merge, Auto-merge when ready, or Ready for review.
  - **Failing checks come first**, with Re-run and View logs buttons always visible. Passing checks fold into one row.
  - Branches without a pull request get a **Create pull request** button and a list of their workflow runs.
  - The tab shows a red, amber or green dot for CI status.
- **Scripts tab.** A new right-sidebar tab lists the scripts in the workspace's `package.json`, plus Install. Each script runs in a new terminal tab using your package manager (bun, pnpm, yarn or npm, detected from `packageManager` or the lockfile). (#108)
- **Branch-tree project sidebar.** Each project now expands into its main checkout and worktrees, with Claude and Codex sessions nested under the branch they run on. Row actions appear on hover, and Add Folder and Clone share one Add menu. The project dialog no longer shows the startup command and task fields, which never ran; the Scripts tab replaces them. (#108)

## Improvements

- Discarding a file, reverting a commit and dropping a stash now ask for confirmation first. (#115)
- Stashing now includes untracked files. (#115)

## Bug Fixes

- Git errors now appear in the app. Commit (including a failing pre-commit hook), amend, branch switch, stash and discard used to fail silently while the sidebar reported success. (#115)
- "Merge now" is no longer offered on a PR with failing or pending checks unless you choose to bypass rules. Auto-merge no longer fails when "Bypass rules (admin)" is ticked. (#115)
- Fixed a crash in the GitHub sidebar's check list when a check had been re-run, or had been triggered by both a push and a pull request. A check that failed and then passed on re-run no longer counts as failing. (#110)
- Projects that aren't git repositories no longer show a made-up `main` branch row, whose worktree button failed. (#112)
- The active project in the sidebar can now be collapsed. (#112)
