# Release v0.40.3

**Released:** 2026-10-06
**Previous version:** v0.40.2

Chat now asks for approval only when Claude Code itself would.

## Bug Fixes

- Fixed chats asking to approve tool calls that the terminal would have settled on its own, especially in auto mode, where chat asked before auto mode's classifier could approve the call. Chat now shows an approval only when Claude Code is about to show its own permission dialog, so Bypass, auto mode and your permission rules behave the same in chat as in the terminal. Always allow in chat now adds a real session rule. (#189)
