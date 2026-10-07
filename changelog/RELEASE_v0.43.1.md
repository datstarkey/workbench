# Release v0.43.1

**Released:** 2026-10-07
**Previous version:** v0.43.0

Chats start again. v0.43.0 shipped the Workbench plugin without one of its files, so no Claude chat could start.

## Bug Fixes

- Fixed Claude chats failing to start with "hooks module did not load: cannot import "./jobs"". The plugin Workbench writes for each session was missing `jobs.ts`, so the plugin didn't load. Tests now fail if a plugin file or one of its imports is left out of the build. (#219)
