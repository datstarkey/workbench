# Release v0.50.0

**Released:** 2026-10-09
**Previous version:** v0.49.0

Workbench now has exactly one way to start, restart and end a session. The older routes the desktop and phone used before v0.49.0 are gone, a session can never run twice, and a retried start never opens a second tab. The phone needs Workbench v0.49.0 or later on the computer it connects to.

## Breaking Changes

- The phone app now requires the desktop (or `workbench-server`) to be on v0.49.0 or later; an older one shows "Update Workbench on your computer". Update the desktop first (#273)
- The old session routes are removed: `POST`/`DELETE /remote/terminals`, the `POST /agent/claude` and `/agent/codex` starts, `DELETE /agent/…`, the per-kind agent lists and `/events/home`. Use `POST /workspace/commands` and `GET /events/workspace` instead; `GET /agent` and `GET /remote/terminals` stay for reading (#273)

## Improvements

- A session can never run twice: restarts, rewinds and mode switches from any device wait for each other, and resuming a conversation right after `/clear` opens the existing tab (#273)
- Starting a session from the phone over a slow connection no longer risks a duplicate when the request is retried (#273)
- A chat whose session has ended says "This session isn't running" with a Restart button straight away, instead of reconnecting for up to a minute (#273)
- On Windows, a notice explains when an agent action's prompt could not be passed to the terminal (#273)
