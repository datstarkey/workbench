# Release v0.32.0

**Released:** 2026-10-01
**Previous version:** v0.31.0

A phone-and-chat release. The Android app can pair with several machines and switch between them, chats started on the phone now show up on the desktop, and chat mode on both shows your 5-hour and weekly plan usage next to an accurate context meter. The agents panel is easier to navigate, finished tool calls take up far less room, and the phone's composer, reconnects and repo links all got attention.

## New Features

- **Several machines on the phone.** Scanning a machine's QR code now adds it instead of replacing the last one, so the phone can hold your MacBook and your PC side by side. The server pill on the home screen opens a switcher to rename, switch to, add or forget a machine. A switch checks the new machine first and keeps you connected if it's unreachable, and your existing pairing carries over. (#136)
- **Phone chats on the desktop.** A chat started on the phone now opens as a background tab in the matching desktop workspace, attached to the same conversation. Closing or restarting that tab leaves the phone's chat running. (#133)
- **Plan usage in chat.** Chat mode on desktop and phone shows `5h` and `Week` usage chips next to the context meter, in the warning colour from 80%; tap or hover for the reset time. They refresh after each turn, use each chat's own Claude account, and the status-bar account menu now shows the same figures. (#137)
- **GitHub link on the phone.** Expanding a project on the phone's home screen shows its GitHub repo; tap to open it. (#132)
- **Check for updates button.** The activity rail has a download button just above Settings to check for updates, on every platform. (#134)

## Improvements

- **Agents panel.** Subagents and background jobs are now separate tabs. Pick one from the list to see its details and live output, so switching between subagents is one tap — on desktop and on the phone. (#131)
- **Finished tool calls fold away.** Runs of finished tool calls collapse into a single row, such as "✓ 6 tool calls · Bash, Read", that expands on tap. Running and failed calls still show in full. (#131)
- **Phone composer.** Enter now starts a new line on the phone, and the send button sends. The permission, model and effort pickers no longer wrap onto two lines on narrow screens. (#131)

## Bug Fixes

- The context meter showed sessions on a 1M-token model as five times fuller than they were (for example 63% instead of 13%). (#131)
- After unlocking the phone, a chat no longer gets stuck reconnecting; you no longer have to leave and reopen it. (#131)
- The automatic update check at startup could close an update dialog you had just opened. (#134)
