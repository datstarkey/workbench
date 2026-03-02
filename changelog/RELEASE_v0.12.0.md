# Release v0.12.0

**Released:** 2026-03-02
**Previous version:** v0.11.0

This release makes the Git and GitHub sidebars noticeably faster at picking up changes, adds a live activity log for hook scripts so you can see exactly what ran and when, and fixes a visible PowerShell window flash on Windows when hooks fire.

## New Features

- **Hook activity log in Settings** -- The Settings > Hooks tab now includes a collapsible "Recent Activity" section that shows a live feed of every hook event and script execution. Each entry displays a timestamp, source badge, and summary message. Errors are highlighted so they stand out, and an error count badge appears on the section header when something goes wrong. You can clear the log at any time. This makes it much easier to verify that your hooks are running correctly and to diagnose failures without digging through system logs.

## Improvements

- **Faster Git sidebar updates** -- The Git and GitHub sidebars now reflect file changes almost instantly instead of after a noticeable delay. The file watcher debounce was reduced from 500ms to 50ms, and the refresh pipeline now fires on the leading edge of a change (within 300ms) rather than waiting for activity to stop. Staging and unstaging files in the Git sidebar is particularly faster because the watcher now monitors `.git/index` directly, so the staged/unstaged file list updates as soon as you act.

- **Faster GitHub sidebar updates after agent activity** -- When Claude or Codex finishes making changes, the GitHub sidebar now fetches fresh PR and CI status directly rather than waiting for the next polling cycle. A staleness check also prevents redundant fetches, so updates are both faster and more efficient.

- **General performance improvements** -- Hot paths across the frontend and Rust backend were optimized, and shared UI patterns were extracted into reusable components, reducing the overall bundle and improving rendering performance throughout the app.

## Bug Fixes

- **Fixed PowerShell window flashing on Windows when hooks run** -- On Windows, every hook event caused a PowerShell console window to briefly appear and disappear, which was distracting during normal use. Hook scripts now run with `-WindowStyle Hidden` and `-NoProfile` flags, so they execute silently in the background.
