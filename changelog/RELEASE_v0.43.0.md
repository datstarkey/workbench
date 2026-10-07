# Release v0.43.0

**Released:** 2026-10-07
**Previous version:** v0.42.0

You can now update your desktop from the Android app. Claude chats keep questions answerable on your phone, switch models for the current session only, and show plan usage live from the running session. Update desktop and Android together.

## New Features

- Added host updates from Android. Your machine shows its Workbench version and any available update; confirm, and the desktop installs it and restarts. Running terminals and chats on it end, and the desktop shows that another device is updating it. (#217)
- Changing the model in a Claude chat now applies to that session only and no longer changes your default model. Your pick survives a rewind or a mode change. (#216)
- Forked skills Claude runs now appear in the Agents and tasks panel while they work. (#214)

## Improvements

- Plan usage in Claude chats and the account switcher now comes live from the running session, so it updates as you work. (#216)
- A background job that was stopped now shows as stopped, not completed. (#216)
- New commands and skills appear in a chat's `/` menu without reconnecting, and `/rename` updates the chat title at once. (#216)
- The Claude model list loads instantly from a cache kept between launches and refreshes in the background. (#216)

## Bug Fixes

- Fixed questions and approvals moving to the terminal when your phone went to the background. A question asked in a turn you started from chat now waits for your answer in chat. (#214)
- Fixed ending a chat on one device leaving it open on the other. Ending it on Android now closes the desktop tab, and closing it on desktop closes it on Android. A crash or restart still shows the chat as exited. (#215)
