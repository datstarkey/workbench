# Release v0.40.2

**Released:** 2026-10-06
**Previous version:** v0.40.1

Two chat fixes: Bypass mode now skips approvals in chat as it does in the terminal, and links open once.

## Bug Fixes

- Fixed chats in Bypass mode still asking to approve some tools, such as Claude in Chrome's browser tools, when the same session in the terminal ran them without asking. Chat now leaves the decision to the permission mode in Bypass and Don't ask, and an approval card that does appear shows why Claude Code asked. (#187)
- Fixed a link clicked in a desktop chat opening twice in the browser. (#186)
