# Release v0.5.0

**Released:** 2026-02-13
**Previous version:** v0.4.1

This release adds a dedicated GitHub Actions sidebar for monitoring CI pipeline status in real time, and fixes a longstanding issue where Escape and Ctrl+C could feel unresponsive when running interactive terminal programs like vim, htop, or fzf.

## New Features

- **GitHub Actions sidebar with live CI status** -- A new collapsible sidebar lets you monitor GitHub Actions check runs directly inside Workbench. When you open the sidebar and select a workspace with an associated pull request, you will see the PR title, state, review status, and a detailed breakdown of every CI check -- grouped by status (failing, pending, passing, skipped). Each check shows its workflow name, duration, and a link to open it on GitHub. The sidebar uses adaptive polling: checks refresh every 15 seconds while any are still pending, then slow down to every 90 seconds once everything has settled. The sidebar's open/closed state is persisted across sessions, and you can toggle it from the workspace tab bar.

## Bug Fixes

- **Fixed Escape and Ctrl+C feeling unresponsive in interactive terminal programs** -- When running TUI applications like vim, htop, or fzf, pressing Escape or Ctrl+C could feel sluggish or have no effect. This happened because xterm.js was intercepting these keys for its own purposes (clearing text selections for Escape, copying to clipboard for Ctrl+C) instead of forwarding them to the running process. Both keys are now sent directly to the PTY, and the PTY writer flushes after every write to ensure single-byte inputs like these are delivered immediately.
