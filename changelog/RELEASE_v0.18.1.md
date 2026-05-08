# Release v0.18.1

**Released:** 2026-05-08
**Previous version:** v0.18.0

Patch release fixing a startup crash on macOS and refining terminal input behavior in Claude and shell panes.

## Bug Fixes

- Fixed a crash on app launch when any saved Claude session contained an emoji or accented character near the 80-character mark of its first message. Session label truncation now respects UTF-8 character boundaries (#61).
- Reworked Shift+Enter handling in terminals: Claude panes now send ESC+CR (matching VS Code's terminal binding) for cleaner multi-line input, while shell panes keep bracketed-paste newlines so zsh/bash multi-line editing still works (#60).
- Copied text from Claude and Codex panes no longer carries trailing whitespace from background-painted rows. The trim is scoped to AI panes only and is CRLF-safe so shell copies are unaffected (#60).
