# Release v0.38.0

**Released:** 2026-10-05
**Previous version:** v0.37.0

Claude chats now show how long their prompt cache stays warm, and each chat can keep it warm or compact itself before it expires, so coming back to a long conversation doesn't re-send the whole thing. Claude in Chrome now works in chat mode, ending a chat on the phone closes it on the Mac, and the Android app now reports errors to Sentry.

## New Features

- **Prompt cache timer.** A chip in the chat header (desktop and phone) shows how long Claude's prompt cache stays warm, with a ring that drains as it runs down; it turns amber near the end and shows Cold once expired. Its menu has **Refresh now**, **Keep warm** for 1–8 hours, and **Compact before it expires** (for conversations over 30k tokens). Each chat keeps its own settings, and the server applies them even with the app closed. Keep-alive refreshes show as a single "Cache kept warm" line and use a little plan usage each. (#159)
- **Compact first.** When the cache has expired on a large conversation, a hint above the composer shows how many tokens the next message will re-send and offers to compact first. (#159)

## Improvements

- **Android error reporting.** The Android app now reports crashes and errors to Sentry, including diagnostics from the notification service (stalled polling, alerts blocked in Android settings). This will help track down why notifications aren't arriving. No tokens, server addresses or chat titles are sent. (#160)

## Bug Fixes

- Fixed Claude in Chrome not loading in chat mode when it's turned on by default for your account; it already worked in terminal tabs. (#161)
- Fixed a chat ended from the phone staying open on the Mac as "ended". Its tab now closes. (#158)
