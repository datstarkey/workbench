# Release v0.9.0

**Released:** 2026-02-18
**Previous version:** v0.8.0

This release adds full Windows platform support, eliminates the Python 3 runtime dependency from hook scripts, and delivers noticeably faster terminal responsiveness. Git and GitHub state now refresh instantly when Claude or Codex modifies files, rather than waiting for the next polling cycle.

## New Features

- **Windows platform support** -- Workbench now builds and runs on Windows. Terminals default to PowerShell (or COMSPEC), Windows-specific environment variables are passed through correctly, and path handling supports backslashes and drive letters throughout. The hook bridge uses TCP on Windows with PowerShell 5.1 scripts (pre-installed on all modern Windows systems), so there are no additional runtime dependencies. The installer produces a standard NSIS package, and CI now builds for both macOS and Windows on every release.

- **Instant git and GitHub refresh via hooks** -- Workbench now installs a PostToolUse hook that triggers whenever Claude or Codex runs a Bash command. Instead of waiting up to 90 seconds for the next poll cycle, git branch state, GitHub PR status, and CI check results refresh immediately after tool use. This means sidebar information stays current as you work, without manual refreshing.

- **Trello merge actions moved to the backend** -- Merge-related Trello automations now run in the Rust backend, improving reliability and eliminating frontend-to-CLI round-trips for board operations.

## Improvements

- **Faster terminal rendering** -- Terminal output now uses forced offscreen rendering and write backpressure, reducing dropped frames and input lag when Claude or Codex produces large amounts of output. Typing and scrolling should feel noticeably smoother during heavy terminal activity.

- **Python 3 is no longer required** -- Hook scripts have been rewritten from Python to native bash (Unix) and PowerShell (Windows). On Unix, the bash scripts use the built-in `/dev/tcp` for TCP connections, requiring no external tools. This removes the only non-bundled runtime dependency Workbench had, simplifying installation on systems without Python 3.

## Bug Fixes

- **Fixed hook settings not supporting nested commands** -- Hook configurations that used nested command structures (such as commands with arguments containing spaces) were not parsed correctly, which could prevent hooks from firing. These are now handled properly.

- **Fixed legacy Python hook entries not being cleaned up** -- After upgrading from a previous version, leftover Python hook references in Claude settings files could cause warnings or prevent new hooks from registering. Legacy entries are now detected and normalized automatically during startup.
