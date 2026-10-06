# Release v0.39.0

**Released:** 2026-10-05
**Previous version:** v0.38.0

A Claude chat is now the real Claude Code running in a terminal, so switching a pane between Terminal and Chat is instant and nothing restarts. Chats get everything an interactive session has, including Artifacts and `/design`, and work with the sandbox runtime turned on. A chat started on the phone now shows up on the Mac even when its project isn't open.

## New Features

- **Terminal and chat are one session.** Every Claude chat runs as the interactive `claude` in a terminal; the chat view is a live view of it, bridged by the Workbench plugin. Switching a pane between Terminal and Chat no longer restarts anything, and what you type in either shows up in both. Approvals, questions and plan approvals can be answered from chat (or in the terminal when no chat is open), and model, permission mode and effort can be changed from the chat. Images and files you attach in chat are handed to Claude as file mentions. (#167)
- **Artifacts and `/design` in chat.** Because a chat is a real interactive session, Artifacts and Claude Design's `/design` now work there. (#167)
- **Chat with the sandbox runtime on.** Claude chat is no longer blocked while the sandbox runtime is enabled: the chat's terminal is what the sandbox wraps. (#167)
- **Phone chats always appear on the Mac.** A chat started on the phone used to show up only once its project's workspace was open; Workbench now opens that workspace in the background and adds the chat to it. It closes again once the chat's tab is closed, and its tab name follows the chat's title. (#168)

## Bug Fixes

- Fixed the worktree create and remove buttons accepting repeated clicks with no sign anything was happening; they now show a spinner and stay disabled until the change finishes. (#165)
- Fixed slash commands typed in chat that Claude answers without echoing them (such as `/design-login`, or an alias like `/design consent`) staying as a grey pending bubble. (#164)
- Fixed Claude session titles not being restored when a session was resumed. (#163)
- Fixed the resume list and session labels stalling the app on large chat histories: only title entries are read past the first prompt now, off the main thread. (#166)

## Notes

- Model changes made from a terminal-backed chat go through Claude Code's `/config`, the same as changing it in the terminal, so they may become your default.
