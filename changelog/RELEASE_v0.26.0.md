# Release v0.26.0

**Released:** 2026-07-24
**Previous version:** v0.25.0

Terminals move to a shared-PTY architecture: the embedded loopback server now owns every terminal session, and the desktop attaches to it over WebSocket — the same contract used by mobile and remote clients. The headline benefit: terminals survive frontend reloads and reconnect to their running shells instead of losing them.

## New Features

- Shared-PTY terminal attach — desktop terminal panes are now WebSocket clients of the always-on embedded loopback server, with automatic port/token discovery. Terminals keep running through app reloads and re-attach with full scrollback replay (#81)
- Single-attacher takeover — attaching to a terminal from a second client cleanly takes over the session: the old view is notified and closed rather than left silently frozen, and only the active attacher's input reaches the shell (#81)
- Terminal panes now receive the shell's real exit code when a session ends, via a dedicated exit control frame (#81)

## Improvements

- Worktree terminals now resolve correctly against the registered project path, so terminals open reliably in git worktrees (#81)
- Codex session setup no longer symlinks Claude skills into `~/.agents` — Codex integration now only touches `~/.codex/config.toml` and the notify bridge (#81)

## Bug Fixes

- Fixed shell teardown so background processes started in a terminal (e.g. a dev server left running with `&`) are terminated with the shell instead of being orphaned, while still capturing the shell's exit code (#81)
- Fixed a lock-ordering deadlock that could freeze an attached terminal socket when its session was killed (#81)
- Fixed leaked terminal sessions on close paths, including when restarting an AI session in a pane (#81)
