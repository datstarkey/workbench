# Release v0.23.0

**Released:** 2026-06-03
**Previous version:** v0.22.0

This release improves stability and crash visibility. Backend (Rust) errors and panics are now reported to Sentry alongside the existing frontend tracking, and a terminal teardown race that produced spurious error reports has been fixed.

## New Features

- Added backend error tracking — Rust panics and backend failures (including background watcher/poller threads) are now captured in Sentry, with the recent log trail attached to each issue for faster diagnosis. Reporting stays off in development and tests (#74).

## Bug Fixes

- Fixed harmless "Session not found" errors being reported when closing or switching terminals. These occurred when the UI synced a terminal that had already shut down; they no longer surface as crashes (#73).
