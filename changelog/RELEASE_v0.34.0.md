# Release v0.34.0

**Released:** 2026-10-02
**Previous version:** v0.33.0

This release makes the phone a fuller workspace for Claude and Codex. Resume saved conversations, review changed files, keep unfinished drafts, and get Android alerts while an open chat works in the background. Usage pills on both desktop and phone now show reset countdowns and context details when tapped.

## New Features

- **Conversation history on the phone.** Added searchable Claude and Codex history for each project and worktree. Resume from Home, the chat's three-dot menu, or `/resume`; Claude history can be filtered by account. (#140)
- **Git review on the phone.** Added a changed-file list with staged and working-tree diffs, including previews of new files. Open it from a project, worktree, or the chat menu. (#140)
- **Android session alerts.** Added opt-in approval and turn-completion notifications for the open conversation. Monitoring continues when the phone locks or Workbench goes into the background, and stops when you return Home, close the chat, switch away, or disconnect. Tap an alert to reopen its conversation. (#140)
- **Codex slash skills.** Added installed Codex skills to slash-command suggestions on desktop and phone, with automatic refresh when available skills change. (#140)
- **Claude account selection.** Added a phone setting to choose the account for new Claude sessions; resumed conversations retain their original account. (#140)

## Improvements

- **Keep unfinished messages.** Text drafts now survive navigation and app restarts, separated by machine, agent, account, and conversation. Image attachments stay available while navigating within the app. (#140)
- **More useful usage pills.** Tap the 5-hour or weekly pill to see how long remains until reset and its date/time. The context pill shows its percentage without the extra label; tap it for current and maximum token counts. All three borders fill according to usage, for both Claude and Codex on desktop and phone. (#140)
- **Chat links and navigation.** Added Open on GitHub to the phone chat's three-dot menu, kept chat links outside the Workbench webview using the phone's default URL handler, and made Android Back dismiss sheets before returning to Home. (#140)

## Bug Fixes

- Failed session and terminal stops now show an error and keep the conversation or terminal visible so you can retry. (#140)
- Fixed a terminal-creation race that could restore a terminal from the previous machine after switching servers. (#140)
- Fixed Git changed-file names containing spaces, quotes, line breaks, or non-ASCII characters, and preserved rename destinations in the file list. (#140)
