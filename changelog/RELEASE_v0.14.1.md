# Release v0.14.1

**Released:** 2026-03-03
**Previous version:** v0.14.0

A stability fix that resolves a crash caused by store initialization order in the app's startup sequence.

## Bug Fixes

- **Fixed a crash on launch caused by incorrect store initialization order** -- Under certain conditions, the app could fail to start with a `lifecycle_outside_component` error. This happened because some stores were accessing other stores at runtime via Svelte context lookups, but those dependencies hadn't been initialized yet. Store references are now resolved at construction time and the initialization order in the app root has been corrected, ensuring all dependencies are available when needed.
