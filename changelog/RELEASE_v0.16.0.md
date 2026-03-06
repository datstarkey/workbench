# Release v0.16.0

**Released:** 2026-03-06
**Previous version:** v0.15.0

This release adds GitHub repository cloning and PR checkout directly from the app, a new native macOS terminal renderer powered by SwiftTerm, and major xterm.js performance improvements modeled after VS Code's terminal architecture.

## New Features

- **Clone repositories from GitHub without leaving Workbench** -- A new Clone button in the sidebar opens a dialog that lists your GitHub repos (via `gh` CLI), lets you search or enter any URL/owner-repo, pick a destination folder, and clone. The cloned project is automatically added and opened. You can set a default clone directory in Settings > Workbench to skip the folder picker.

- **Checkout PRs and open them as worktrees from the GitHub sidebar** -- Open PRs now show Checkout and Open as Worktree buttons in the sidebar PR header. Checkout switches your working copy to the PR branch. Open as Worktree fetches the branch and creates an isolated worktree workspace, so you can review a PR without disrupting your current work.

- **Native macOS terminal renderer via SwiftTerm** -- A new optional renderer routes PTY data directly from Rust to a native SwiftTerm view via FFI, bypassing the webview entirely for terminal I/O. Enable it in Settings > Terminal > Terminal Renderer. The renderer is stamped per-workspace at creation time, so changing the setting won't disrupt live terminals.

- **Overhauled xterm.js terminal performance** -- The default xterm.js renderer now follows VS Code's terminal patterns: split-axis resize debouncing (rows instant, columns 100ms), direct `terminal.write()` with callbacks instead of buffered queues, synchronous fit-and-flush on tab switch, and static WebGL fallback. This fixes resize lag, terminal freezing after commands, and sluggish tab switching.

## Improvements

- **Claude skills are now symlinked into Codex agents** -- Skills in `~/.claude/skills/` are symlinked into `~/.agents/skills/` instead of being deep-copied. Changes to skills are reflected immediately without re-syncing, and the swap is atomic with automatic rollback on failure.

## Bug Fixes

- **Fixed gh CLI auth status shown in settings** -- The Workbench settings page now displays whether the GitHub CLI is authenticated, with a helpful prompt to run `gh auth login` if not.

- **Fixed ScrollArea height constraints** -- Scroll areas that need bounded height now use fixed `h-N` instead of `max-h-N`, which the internal viewport was ignoring.
