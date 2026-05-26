# Release v0.18.2

**Released:** 2026-05-26
**Previous version:** v0.18.1

Patch release that fixes a UI freeze when creating or removing git worktrees.

## Bug Fixes

- Fixed the app freezing whenever a worktree was created or deleted. Worktree git operations (including the network `git fetch` performed during creation) and the post-delete refresh now run on a background thread instead of blocking the UI. (#62)
