# Release v0.45.3

**Released:** 2026-10-08
**Previous version:** v0.45.2

Questions asked in a Claude chat can be answered from the chat again, after Claude Code 2.1.292 changed how plugins handle them. Chats also no longer freeze when a request to Workbench never returns, the permission mode picker sticks, and on the phone, restoring a chat from history no longer freezes the app and a chat can be restarted.

## Bug Fixes

- Fixed questions in Claude chats getting stuck on "Sending…". Since Claude Code 2.1.292, Claude Code opened its own terminal dialog even after you answered in chat, so the answer never reached Claude. Your answer now goes straight to Claude. (#231)
- Fixed a Claude chat freezing when one of its requests to Workbench never returned. Workbench now gives up on a stuck request and retries without losing your answer, so a dropped connection costs a short wait instead of a dead chat. (#231)
- Changed plan approvals to happen in the terminal. Claude Code now only leaves plan mode through its own dialog, so the chat says to approve the plan there, with a Show terminal button, and your phone and desktop still alert you. (#231)
- Fixed the permission mode picker in Claude chats snapping back to the old mode. A pick shows at once and sticks after Claude restarts in the new mode, and picking twice no longer starts a second Claude on the same session. (#232)
- Fixed the phone app freezing when you restored a conversation from a project's history on the home screen. (#233)
- Added Restart session to the phone's chat menu. It restarts Claude for that conversation, a stuck turn included, keeping its mode and model, as Restart does on desktop. (#233)
- Fixed a Claude chat showing a 200k context window and a 5-minute cache timer until its first turn ended, instead of its 1M context window and 1-hour cache. (#230)
