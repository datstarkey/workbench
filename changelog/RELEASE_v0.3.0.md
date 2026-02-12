# Release v0.3.0

**Released:** 2026-02-11
**Previous version:** v0.2.0

Workbench now has a native menu bar with built-in update checking, so you'll always know when a new version is available. This release also fixes a couple of UI issues that affected worktree workflows and the settings panel.

## New Features

- **Native menu bar with auto-update support** -- Workbench now includes a proper macOS menu bar with standard Edit and Window menus, a Settings shortcut (Cmd+,), and a "Check for Updates" action. When an update is available, a dialog shows release notes and lets you download and install it with one click. The app also checks for updates automatically on startup so you never miss a release.

## Bug Fixes

- **VS Code now opens at the correct path for worktrees** -- Previously, clicking the "Open in VS Code" button while working in a git worktree would open VS Code at the parent project root instead of the worktree directory. It now correctly opens the worktree path, matching the directory your terminals are running in.

- **Fixed save button overlap in the settings panel** -- The save and close buttons in the Claude Code Settings sheet no longer overlap with the panel's close icon, preventing accidental mis-clicks.
