# Release v0.4.1

**Released:** 2026-02-12
**Previous version:** v0.4.0

A quick patch release that fixes two issues introduced with the v0.4.0 release. Clickable links in the terminal now open correctly in your system browser, and the GitHub integration sidebar badges work reliably across all projects and worktrees.

## Bug Fixes

- **Fixed clickable links in the terminal opening inside the app instead of your browser** -- URLs displayed in terminal output (such as links printed by CLI tools or build output) were opening inside the Tauri webview rather than launching your default system browser. Clicking any link in the terminal now correctly opens it in your browser as expected.

- **Fixed GitHub PR badges not appearing in the sidebar** -- The GitHub integration added in v0.4.0 was referencing incorrect property and method names on the GitHub store, which prevented PR status badges from loading in the project sidebar. Badges now appear correctly for both main project branches and worktree branches.
