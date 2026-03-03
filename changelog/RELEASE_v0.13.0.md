# Release v0.13.0

**Released:** 2026-03-03
**Previous version:** v0.12.0

This release adds support for the Happy Coder CLI, letting you route all Claude sessions through the `happy` binary for remote mobile access to your Workbench projects.

## New Features

- **Happy Coder integration for remote sessions** -- A new "Use Happy Coder" toggle is available in Workbench Settings under the Features section. When enabled, all Claude session commands -- new sessions, resumed sessions, and agent actions -- use the `happy` CLI instead of `claude`. This lets you access your Workbench terminals remotely from a mobile device via the Happy Coder service. The setting is global and persists across restarts. Codex sessions are unaffected and continue to use the `codex` binary as before.
