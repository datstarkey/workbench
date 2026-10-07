# Release v0.45.0

**Released:** 2026-10-07
**Previous version:** v0.44.0

Ending a session now ends it on every device, and the phone's running list is grouped by workspace.

## New Features

- Grouped the phone's Running list by workspace. Chats and terminals sit under the project or worktree they run in, named as in the project list (`project · branch`), and groups keep their place as sessions update. (#224)

## Bug Fixes

- Fixed closing a chat on the desktop leaving it open on the phone. Closing a tab, pane or whole workspace now ends its sessions everywhere, including ones started on the phone, and closing a terminal running Claude from the phone closes it on the desktop too. (#224)
