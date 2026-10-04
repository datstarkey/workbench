# Release v0.36.0

**Released:** 2026-10-04
**Previous version:** v0.35.1

This release fills the biggest gaps in the native chat. You can attach PDFs and text files, mention project files with `@`, rewind a chat to before any prompt, and answer questions from MCP servers. Artifacts, blocked tools and failing hooks now show in the chat, and the Android app notifies you about every session, not just the one you had open.

## New Features

- **Attach PDFs and text files.** Claude chats accept PDFs (up to 10 MB) and text files of any type (up to 256 KB), up to 5 per message, by picker, paste or desktop drag-drop. They show as name chips on your message. Codex chats stay images-only, because Codex can't read documents. (#149)
- **Mention project files with `@`.** Typing `@` in the composer opens a filtered list of the project's files; picking one inserts `@path`, and Claude reads the file itself. Works in Claude and Codex chats. (#149)
- **Rewind a chat.** Each of your prompts in a Claude chat has a rewind button. It previews which files would change, then restores the code, the conversation, or both to before that prompt, like the CLI's `/rewind`. Chats started before this release can rewind only the conversation. (#150)
- **Answer MCP questions.** When an MCP server asks for input mid-turn, a card shows its form or link, with Accept, Decline and Not now. Works in Claude and Codex chats, and on the phone it opens in the waiting sheet. (#152)
- **Artifacts in the chat.** Artifacts Claude creates or updates get their own card with an open link, and the chat header lists every artifact from the conversation. (#151)
- **Suggested next prompt.** After a turn, Claude's suggested follow-up appears as a chip above the composer; tap it to fill the draft. (#151)

## Improvements

- **See what happened behind the scenes.** The chat now shows when auto mode blocks a tool, when a hook blocks or fails, which memory files Claude recalled, and when a model refuses or falls back. Hooks that run cleanly stay silent. (#151)

## Bug Fixes

- Fixed Android notifications only covering the chat that was open on screen. The phone now watches every Claude and Codex session on the connected machine, and catches turns that finish between two checks. (#148)
