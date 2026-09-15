# Release v0.29.0

**Released:** 2026-09-15
**Previous version:** v0.27.2

v0.28.0 was never published (its Windows build failed on a GitHub download timeout), so this release carries everything since v0.27.2.

Two themes. First, running Claude with fewer prompts without giving up the safety net: Claude Code v2.1.257 stopped honouring `bypassPermissions` and `auto` from a project's `.claude/settings.json`, so Workbench now owns the permission mode, and a new opt-in sandbox runtime confines the whole Claude process. Second, the phone becomes a real client: terminals are shared between the desktop and the Android app, you pair the phone by scanning a QR code, and the Android app now ships with every release and updates itself without the Play Store. Server mode now always requires a token.

## Breaking Changes

- Server mode now always requires a token, and the desktop's local terminal server uses one too. Workbench generates the server-mode token for you (Settings → Workbench → Server mode). **A phone that was connected to server mode stops connecting until you pair it again** — scan the QR code, or paste the token (#100)
- The standalone `workbench-server` binary refuses to listen on a non-loopback address without `--token` (pass `--insecure-no-token` to opt out deliberately), and rejects tokens shorter than 32 characters (#100)
- Removed the Happy Coder integration. The "Happy Coder" toggle is gone and Claude sessions always launch the `claude` CLI. Existing settings files that still contain the old `useHappyCoder` key load unchanged and the key is dropped on the next save (#93)

## New Features

- **Shared terminals across desktop and phone.** Terminals opened on the desktop show up in the Android app, and terminals opened on the phone appear as tabs in the matching desktop project. Either device can **Take control** of a terminal the other is using; neither reconnects on its own, so they never fight over a session. Closing a phone's terminal tab on the desktop only detaches it, and those tabs are not restored after a restart (#100)
- **Pair the phone with a QR code.** Settings → Workbench → Server mode → **Pair phone** shows a QR code with an address picker (Tailscale addresses first) and a warning when the chosen address is plain HTTP on a non-Tailscale network. In the Android app, **Scan QR code** fills in the server and token and connects. The scan can be cancelled with the on-screen button or the back button (#101)
- **Android app releases.** Every release now includes a signed `workbench-android-<version>.apk` (64-bit ARM) alongside the macOS and Windows builds, and the app has proper Workbench launcher icons (#102)
- **In-app updates for Android, no Play Store.** The app checks for a newer release on launch and shows an update banner. Tapping **Update** downloads the APK, verifies its checksum and opens Android's install prompt. If the download finishes while the app is in the background, it offers **Install** when you return instead of downloading again. Android only accepts updates signed with the same key as the installed app (#102)
- Server mode settings show the token masked, with reveal, copy and **Regenerate**. Regenerating (or turning server mode off) disconnects every device that was using the old token; the desktop's own terminals are unaffected (#100)
- Added a **Claude permission mode** setting (Settings → Workbench). Choose Default, Accept Edits, Plan, Don't Ask, Auto or Bypass Permissions and Workbench appends `--permission-mode <mode>` to every new and resumed Claude session. This restores per-mode launches after Claude Code v2.1.257 stopped reading `bypassPermissions` and `auto` from project-scope settings. Changing the mode updates the saved startup command of every open Claude pane, including ones started with an initial prompt. Choosing Bypass shows a warning that it skips every permission check (#93)
- Added an opt-in **sandbox runtime** for Claude sessions (Settings → Workbench, macOS and Linux). When on, every Claude launch and resume is wrapped in Anthropic's `@anthropic-ai/sandbox-runtime`, so the whole `claude` process runs behind the operating-system sandbox: file tools, MCP servers and hooks included. Sandboxed sessions can write to your project folders, `/tmp` and Claude Code's own session data, and nothing else. Hooks, skills, agents, plugins, MCP server config, git hooks and Workbench's own settings are read-only, so a session cannot install code that would run unsandboxed later. Credential directories such as `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.kube` and `~/.docker` are unreadable. Network access is limited to Claude Code's required hosts plus a domain allowlist you control. Needs Node (`npx`); Linux also needs `bubblewrap`, `socat` and `ripgrep`. The first launch downloads the runtime from npm (#94)
- Workbench has a product page at [workbench.starkeydigital.com](https://workbench.starkeydigital.com/) with links to the latest downloads (#99)

## Improvements

- Hardened the control-plane server: terminal WebSocket connections from browser pages other than the Workbench apps are rejected, CORS is restricted to the headers the apps use, tokens are kept out of spawned shells, settings responses and error messages (#100)
- Project startup commands and agent tasks that launch `claude` now honour the permission mode and sandbox settings too. A permission flag you wrote yourself in a startup command is left as is (#94)

## Bug Fixes

- Android: terminal output can now be scrolled by dragging. xterm.js 6 had dropped touch scrolling; drags now scroll the scrollback, and move line by line in full-screen apps such as vim, while a tap still focuses the terminal (#97)
- Workbench no longer accumulates zombie processes over long sessions. "Open in VS Code" and opening links left a finished `open` process behind every time, and killed or exited remote Claude sessions were never reaped (#96)
- Windows: startup commands now run in the shell panes actually use. The v0.27.1 fix landed in a code path Windows never hits, and the command was also sent before the console shell had finished initialising, so it was silently swallowed. Commands are now submitted once the shell is ready, with the correct line ending (#92)
