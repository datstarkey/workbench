# Release v0.40.1

**Released:** 2026-10-06
**Previous version:** v0.40.0

A fix for starting a chat in chat mode.

## Bug Fixes

- Fixed starting a chat in chat mode also opening an extra terminal tab with a "Terminal opened on another device: terminal" notice. The chat's own terminal was mistaken for one opened on another device while Claude was still starting. (#183)
