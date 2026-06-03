# Release v0.22.0

**Released:** 2026-06-03
**Previous version:** v0.21.0

This release adds production crash reporting and fixes the recently-introduced Settings window, which could appear as a blank screen or flash white before loading.

## New Features

- Added crash reporting for production builds, so errors are captured and fixed faster. It is disabled entirely during local development, so nothing is reported while building or running the app yourself (#69).

## Bug Fixes

- Fixed the Settings window appearing as a blank black screen and never loading its contents (#71).
- Fixed a brief white flash when opening the Settings window — it now stays hidden until its content has rendered (#72).
