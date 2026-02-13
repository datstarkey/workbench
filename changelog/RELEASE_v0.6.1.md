# Release v0.6.1

**Released:** 2026-02-13
**Previous version:** v0.6.0

A patch release that fixes GitHub integration being completely non-functional in production (bundled) macOS builds. If you were running Workbench from the built .app and noticed that the GitHub sidebar was empty or unresponsive, this update resolves that.

## Bug Fixes

- **Fixed GitHub integration not working in bundled macOS app** -- When Workbench is launched as a bundled macOS application (rather than via `bun run dev`), macOS provides a minimal system PATH that excludes directories where tools like the GitHub CLI (`gh`) are typically installed -- such as Homebrew (`/opt/homebrew/bin`), Nix, and MacPorts locations. This caused every `gh` command to silently fail, which disabled the entire GitHub integration sidebar including PR listings, CI status badges, and workflow run details. Workbench now enriches the PATH for all GitHub CLI invocations to include common tool installation directories, so `gh` is found regardless of how the app was launched.
