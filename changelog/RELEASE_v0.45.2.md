# Release v0.45.2

**Released:** 2026-10-07
**Previous version:** v0.45.1

Stop in a Claude chat now also ends the background agents the chat started, such as the one a `/code-review` runs.

## Bug Fixes

- Fixed Stop not ending a chat's background agents. Stopping now ends each one at its next step, marks its task as stopped, and starts no follow-up turn, so a stopped agent uses nothing more. Stop also shows while only a background agent is running, which a forked skill like `/code-review` leaves behind. (#228)
