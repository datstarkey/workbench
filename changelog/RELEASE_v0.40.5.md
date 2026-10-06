# Release v0.40.5

**Released:** 2026-10-06
**Previous version:** v0.40.4

Slash commands that don't start a model turn now work in chat, including forked skills like `/code-review` and `/compact`.

## Bug Fixes

- Fixed chat hanging on "thinking" after a slash command that runs no model turn, such as `/cost`, `/context` or a forked skill like `/code-review`. The command's output now shows in chat and the chat goes idle. (#197)
- Fixed a forked skill's background agent (e.g. `@code-review`) not appearing in the chat's tasks panel. It now shows with live output and closes when the agent finishes. (#197)
- Fixed `/compact` in chat hanging and never showing "Conversation compacted". This also fixes the Compact first hint and compact-on-expiry. (#197)
