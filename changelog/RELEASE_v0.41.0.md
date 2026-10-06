# Release v0.41.0

**Released:** 2026-10-06
**Previous version:** v0.40.5

Desktop and Android now share more of the Claude and Codex chat experience, with Android dictation, file attachments, consistent notifications, and expanded Codex controls. Update desktop and Android together for the new notification connection.

## New Features

- Added voice-to-text input to Claude and Codex chats on Android. The microphone button inserts recognized speech into your draft so you can edit it before sending. (#200)
- Added Codex chat controls on desktop and Android for compacting, reviewing, forking, browsing history, managing follow-up queues and goals, and accessing account and MCP setup actions supported by your installed Codex. (#153)
- Added host nicknames on Android. Name a host during pairing or rename any saved host, including one that's offline, without interrupting a chat. (#202)
- Added **Settings → General → Start on startup** on Windows and macOS to launch Workbench when you sign in. (#199)

## Improvements

- Added matching Claude and Codex logos throughout desktop and Android, including launch buttons, tabs, session lists, chat headers, settings, and history pickers. (#203)
- Unified chat attachments for Claude and Codex: both now accept images, PDFs, and text files through the same composer. (#201, #153)
- Improved Codex session cleanup and handling of stalled requests, large outputs, and configuration updates. Existing notification commands are preserved when configuring Workbench integration. (#153)

## Bug Fixes

- Fixed switching a Claude chat to Terminal on Android starting another session. Both views now show the same running Claude session, and the home screen avoids duplicate entries. (#201)
- Fixed missing and duplicate chat notifications by delivering the same session events to desktop and Android, including quick Codex completions and approval requests. Codex terminal notifications now use its notify integration correctly. (#200, #201)
- Fixed tapping a Codex terminal notification starting a separate chat process; it now opens the existing terminal. (#201)
- Fixed Codex launch settings differing between desktop and Android, and queued file attachments losing their references after editing. Rejected queue submissions now preserve the draft and attachments. (#201, #153)
