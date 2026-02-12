# Release v0.4.0

**Released:** 2026-02-12
**Previous version:** v0.3.0

This release brings GitHub integration directly into Workbench. You can now see pull request status and CI checks at a glance in the sidebar, and jump straight to GitHub from any workspace. This release also fixes a terminal input issue and improves tab bar behavior when you have many workspaces open.

## New Features

- **GitHub integration with PR status and CI checks** -- If you have the `gh` CLI installed, Workbench now shows pull request badges next to branches in the sidebar. Each badge displays the PR number, its state (open, draft, merged, or closed), and a live CI status indicator that shows whether checks are passing, failing, or still running. Badges update automatically every 90 seconds for open PRs and refresh immediately when you switch branches. You can click any badge to open the pull request in your browser. A new GitHub button also appears in the workspace tab bar and project context menu, giving you one-click access to the repository or current branch on GitHub.

## Improvements

- **Workspace tabs now wrap gracefully when space is tight** -- When you have many workspaces open, the tab bar now wraps to multiple lines instead of overflowing or clipping tabs. Every workspace tab stays visible and clickable regardless of how many you have open.

## Bug Fixes

- **Fixed input lag when pressing Shift+Enter in the terminal** -- Pressing Shift+Enter to insert a newline in the terminal could cause noticeable input latency and occasionally duplicate characters. The newline sequence is now written directly to the PTY, bypassing xterm's paste pipeline entirely. Shift+Enter should now feel as instant as any other keypress.
