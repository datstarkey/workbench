# Release v0.40.4

**Released:** 2026-10-06
**Previous version:** v0.40.3

Chat can now trust a new folder and switch permission modes properly, and it no longer changes your Claude Code settings behind your back.

## New Features

- Added a "Trust this folder?" card to chat. A chat in a folder Claude Code hadn't been trusted in used to fail after 30 seconds, because Claude Code's trust prompt waited unseen in the chat's terminal. Chat now asks you, and Trust folder starts the chat, with Claude Code recording the trust as it does in the terminal. (#191)

## Bug Fixes

- Fixed the chat's mode picker not changing the mode, and Bypass not working from chat at all. Switching mode now restarts the chat's Claude in the new mode and keeps the conversation (not while a turn is running). (#193)
- Fixed picking a mode in chat overwriting the default permission mode in your Claude Code settings (`~/.claude/settings.json`). If you changed modes from chat before this release, check `permissions.defaultMode` there. (#193)
- Fixed rewinding or switching mode sometimes starting a second Claude for the same chat. (#193)

## Improvements

- CI now compiles the macOS-only code on every Rust change, after a missed macOS compile error held this release back. (#195)
- The Workbench Claude Code plugin is now type-checked and linted in CI, which caught and fixed several type errors. (#192)
