# Release v0.14.0

**Released:** 2026-03-03
**Previous version:** v0.13.0

This is a feature-rich release focused on making terminals more powerful, organizing projects at scale, and giving you deeper control over your Git workflow without leaving the sidebar. Highlights include in-terminal search, shell integration with command tracking, project groups with quick filtering, and a fully interactive Git commit log.

## New Features

- **In-terminal search** -- Press Ctrl+F (or Cmd+F on macOS) to open a floating search bar inside any terminal pane. Search results are highlighted incrementally as you type, with a match counter showing your position. Toggle case sensitivity, regex mode, and whole-word matching from the search bar. Navigate between results with the arrow buttons or keyboard shortcuts.

- **Shell integration with command tracking** -- Terminals now automatically detect command boundaries using OSC 133 shell integration sequences. Each command gets a colored gutter decoration: green for successful commands, red for failures. Navigate between prompts with Ctrl+Shift+Up and Ctrl+Shift+Down. For plain Zsh terminals, integration hooks are injected automatically -- no manual shell configuration needed.

- **Font ligature support** -- If you use a ligature-capable font like JetBrains Mono or Fira Code, terminals now render programming ligatures (arrows, comparisons, etc.) correctly.

- **Project groups and quick filter** -- Projects in the sidebar can now be organized into named groups that display as collapsible sections. Assign groups through the project dialog, the right-click "Move to Group" submenu, or the inline "New Group" option. A search bar at the top of the sidebar filters projects by name or group name, making it easy to find what you need in large project lists.

- **Interactive Git commit log** -- The Git sidebar now includes a scrollable commit history. Expand any commit to see its full message, copy its SHA to the clipboard, or revert it directly from the sidebar. Amend the most recent commit without switching to the terminal.

- **Branch creation from the Git sidebar** -- Create new Git branches directly from the sidebar's branch section, without needing to open a terminal.

- **Stage all / unstage all for Git changes** -- Bulk stage or unstage all modified files with a single click in the Git status section.

- **Hook activity log now shows commands** -- Hook log entries for Bash tool-use events now display the actual command that was executed (truncated to 80 characters). When a git or gh command triggers an automatic project refresh, the entry shows a "refreshed" indicator so you can see exactly what caused your sidebar to update.

## Improvements

- **Faster keystroke echo** -- Input-to-display latency has been significantly reduced. Small terminal outputs (under 128 bytes, typical of keystroke echoes) now bypass the animation frame pipeline entirely, resulting in near-zero latency when typing.

- **Reliable fast typing** -- Fixed an issue where typing quickly (especially capital letters or special characters) could cause keystrokes to arrive at the shell out of order. Terminal input is now serialized per session, with regular characters batched via microtask and control characters flushed immediately.

- **Smoother terminal resizing** -- Terminal resize events are now debounced at 500ms (up from 50ms), reducing unnecessary reflows during window resizing and split-pane adjustments.

## Bug Fixes

- **Fixed git status display cutting off the first character** -- The first letter of filenames in the Git sidebar status section was being truncated. This was caused by the status parser incorrectly trimming leading whitespace from Git's porcelain output format, which uses fixed-width status columns. File paths now display correctly.
