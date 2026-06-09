# Release v0.25.0

**Released:** 2026-06-09
**Previous version:** v0.24.0

Workbench is no longer just a desktop app — this release turns it into a small fleet you can drive from anywhere. A new headless control-plane server lets another machine list and create worktrees and spawn `claude remote-control` sessions that show up automatically in the Claude mobile app and on claude.ai. The desktop can host that server itself, connect to a remote one, and switch between machines from the sidebar — and there's now a native Android app with persistent terminals. Also fixes a macOS terminal bug where typed text could be duplicated.

## New Features

- **Headless control-plane server** — run `workbench-server` on any machine to list/create worktrees and spawn `claude remote-control` sessions remotely; those sessions register with Anthropic and appear in the Claude mobile app / claude.ai automatically. Terminal IO never crosses the network — only the control plane (#79).
- **Server mode in the desktop app** — host the same control-plane server in-process from Settings → Server mode (toggle + port), so your laptop can be the server other devices connect to (#79).
- **Remote-client mode** — connect the desktop to a remote workbench-server (URL + optional token) and drive its projects, worktrees, and sessions from a shared sidebar (#79).
- **Instance-aware sidebar** — switch between "This Mac" and any connected remote servers from a dropdown in the sidebar header, each with live connection status (#79).
- **Native Android app** — a Tauri mobile client with xterm.js terminals, an Android extra-keys bar (Esc/Tab/^C/^D/arrows), project search, and auto-reconnect to your last server (#79).
- **Persistent remote terminals** — full PTY terminals over WebSocket with scrollback replay and resume-after-detach, so a remote session survives reconnects; one-tap "claude" launch on spawn (#79).
- **Mobile-friendly web client** — the server serves a self-contained control panel at `/` (list projects, create worktrees, spawn/kill sessions), so the spawn-from-phone loop works today over a private network (#79).

## Bug Fixes

- Fixed an issue on macOS where text you typed into a terminal could be duplicated with extra spacing — most noticeable around the space bar and Shift/Caps Lock. Real pastes and control keys are never affected (#80).
