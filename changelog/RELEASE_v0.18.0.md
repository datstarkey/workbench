# Release v0.18.0

**Released:** 2026-04-17
**Previous version:** v0.17.0

This release adds OS-level desktop notifications so you no longer need to keep Workbench focused to know when Claude or Codex is waiting on you. It also fixes two long-standing terminal frustrations: child dev servers surviving after you close a tab, and Shift+Enter accidentally submitting messages in Claude Code.

## New Features

- **Desktop notifications when AI sessions need attention** — Workbench now fires an OS notification whenever a Claude or Codex pane transitions into awaiting-input state. Clicking the notification focuses the window and jumps to the exact tab. Notifications are suppressed only when you're already viewing that specific pane, so other panes still alert you. (#59)

## Bug Fixes

- **Closing a terminal now kills its entire process tree** — Child dev servers like `vite` and `npm run dev` used to survive after closing the terminal that started them, because only the shell itself was killed. Workbench now sends `SIGHUP`+`SIGTERM` to the whole process group, waits briefly for a graceful exit, then escalates to `SIGKILL`. Windows uses `taskkill /T /F` for equivalent tree-kill behavior. (#58)

- **Fixed Shift+Enter inserting duplicate newlines in Claude Code** — Shift+Enter was inserting a bracketed-paste newline *and* a raw carriage return, which made Claude Code submit in-progress messages early. The fix calls `preventDefault` on every intercepted keystroke (Ctrl+F, Escape, Ctrl+C, Shift+Enter, Ctrl+Shift+Up/Down), matching VS Code's xterm integration pattern. (#57)
