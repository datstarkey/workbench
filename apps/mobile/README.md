# @workbench/mobile

**Status: placeholder — not yet implemented.**

The mobile app is the phone-side client for Workbench server mode. Its purpose is
to let you **spawn Claude sessions and create worktrees from your phone**, then
continue the session in the Claude mobile app (Workbench spawns
`claude remote-control`, which auto-appears there).

## Intended stack

- **Tauri v2 mobile** (iOS/Android) wrapping a thin Svelte app.
- Consumes the shared **`@workbench/control-plane-ui`** package (project list,
  worktree creation dialog, session spawn) — the same UI the desktop sidebar uses.
- Injects an **`HttpTransport`** from **`@workbench/transport`** pointed at a
  running `workbench-server` (typically reached over Tailscale).
- Renders **only** the control-plane sidebar. There is no terminal/xterm here —
  `transport.capabilities.terminalIO` is `false`, so the shared UI hides terminal
  affordances and the "start session" action calls `spawnRemote`
  (`POST /remote/spawn`) instead of creating a local PTY pane.

## Not built yet

This folder intentionally contains only this README and a placeholder
`package.json`. No build is wired. See the repo plan for the full design.
