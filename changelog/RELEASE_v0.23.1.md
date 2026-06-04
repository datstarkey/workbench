# Release v0.23.1

**Released:** 2026-06-04
**Previous version:** v0.23.0

A small stability release that fixes two error-reporting crashes surfaced by the new Sentry integration: a duplicate-key crash in the hook activity list and noisy unhandled promise rejections during terminal teardown.

## Bug Fixes
- Fixed a crash in the hook activity list (Settings → Claude → Hooks) that could occur when two hook events arrived at the same moment with the same summary (#77)
- Stopped benign "Session not found" errors from being reported during terminal teardown, when a pane was resized or closed just after its session had already ended (#75)
