# Release v0.33.0

**Released:** 2026-10-01
**Previous version:** v0.32.0

A Codex release. Chat mode now works for Codex as well as Claude: Codex tabs on the desktop get the same native chat view, and the phone can start a Codex chat on any project and pick up Codex chats running on the desktop. Codex chats show streamed replies, commands and file edits, approval prompts, model and reasoning pickers, and your 5-hour and weekly Codex plan usage.

## New Features

- **Codex chat on the desktop.** Codex tabs get the Terminal | Chat toggle Claude tabs have. Chat shows Codex's replies, commands and file edits as they happen, asks before running anything your Codex settings say to ask about, and switches back to the terminal (`codex resume`) with the conversation intact. Resume, restart and the model and reasoning-effort pickers all work. (#139)
- **Start Codex from the phone.** Each project and worktree on the phone's home screen has a Codex button next to Claude that starts a Codex chat there. Codex chats appear in Running and Needs you alongside Claude's, in Codex's colour, and you can answer their approvals straight from the home screen. Codex chats started on the phone also open as background tabs on the desktop. (#139)
- **Codex modes.** Pick Read only, Auto or Full access for a Codex chat, matching Codex's own `/approvals` presets. A new chat starts in whichever of these your Workbench Codex settings match, otherwise in your `~/.codex/config.toml` settings. (#139)
- **Codex plan usage.** Codex chats show `5h` and `Week` usage chips and a context meter sized to the model's real context window. (#139)
