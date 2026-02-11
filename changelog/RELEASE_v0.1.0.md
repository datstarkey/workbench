# Release v0.1.0

**Released:** 2026-02-11
**Initial release**

Workbench is a desktop terminal manager built with Tauri v2 and Svelte 5. It gives each of your local projects a tabbed workspace with real shell terminals, integrated Claude and Codex AI sessions, and full git worktree support -- all in a single native macOS app.

## New Features

- **Tabbed terminal workspaces** -- Add local project folders and get a dedicated workspace for each one, with multiple terminal tabs backed by real PTY sessions via xterm.js.

- **Claude and Codex session integration** -- Start new Claude or Codex sessions directly from a project workspace. The app automatically discovers past sessions from `~/.claude/` so you can resume previous conversations from the sidebar.

- **Git worktree support** -- Create and manage git worktrees without leaving the app. Each worktree gets its own nested workspace in the sidebar with independent terminals and AI sessions. AI configuration files (`.claude/`, `.mcp.json`) are automatically copied into new worktrees so your setup carries over.

- **Drag-and-drop project and task reordering** -- Rearrange projects and tasks in the sidebar by dragging them into your preferred order.

- **Sidebar session nesting** -- AI sessions and tasks now appear nested under their respective worktree in the sidebar rather than being listed flat under the project, making it much easier to find what you are looking for.

- **Visual distinction between Claude and Codex sessions** -- Claude and Codex sessions use different accent colors in the sidebar and tab bar so you can tell them apart at a glance.

- **Automatic updates** -- The app checks for new versions via GitHub Releases and can update itself in place.

## Bug Fixes

- Fixed VS Code integration on macOS -- the "Open in VS Code" action now works reliably from within the bundled app, even when the `code` CLI is not on the system PATH.

- Fixed terminal rendering corruption that could occur when multi-byte UTF-8 characters were split across data chunks.

- Fixed double-spaced terminal output by correcting the default line height.

- Fixed Shift+Enter behavior in Claude and Codex sessions so it correctly inserts a newline instead of submitting the input.

- Fixed an issue where editing task fields in the sidebar would steal focus from the active terminal.

- Fixed browser autocorrect interfering with branch name input in the worktree creation dialog.

- Fixed Codex session label parsing and tab lookup so resumed Codex sessions display the correct name and can be navigated to reliably.
