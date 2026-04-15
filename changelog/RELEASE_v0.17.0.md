# Release v0.17.0

**Released:** 2026-04-15
**Previous version:** v0.16.0

This release overhauls terminal resize behavior to match VS Code's xterm.js architecture, eliminating resize jank and tab-switch lag. Worktree creation now correctly preserves untracked Claude configuration files.

## New Features

- **Worktree creation now copies untracked Claude files** -- Previously, creating a worktree skipped the entire `.claude/` directory if any files in it were git-tracked, silently dropping untracked files like `settings.local.json`. Now the copy logic queries `git ls-files` and skips only tracked files individually, so untracked configuration is properly carried over. Root-level `CLAUDE.md` is also copied. (#54)

## Bug Fixes

- **Fixed terminal resize jank and tab-switch lag** -- Terminal tabs now use a `visibility: hidden` overlay model instead of `display: none`, so terminals always maintain their real dimensions and don't trigger resize storms on tab switch. ResizeObserver callbacks are batched to one per animation frame, CSS `contain: strict` isolates terminal layout from the DOM tree, and a layout suppression guard prevents spurious resize events during split/close pane operations. (#55)

- **Reduced terminal input latency** -- Simplified the input pipeline from a microtask-buffered coalescing system to direct promise-chain writes, matching VS Code's `processManager.write()` pattern. Each keystroke is sent immediately rather than waiting for a microtask flush. (#55)
