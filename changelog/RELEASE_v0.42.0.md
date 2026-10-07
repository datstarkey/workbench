# Release v0.42.0

**Released:** 2026-10-07
**Previous version:** v0.41.0

Claude chats and terminals now show the same session the same way on desktop and Android. Every Claude launch now goes through one hardened path, and Start on startup opens Workbench minimised. Update desktop and Android together: the server dropped endpoints that older builds still call.

## New Features

- Start on startup now opens Workbench minimised when you sign in. (#211)
- Claude chats now show usage-limit warnings and the context window. They also show auto mode's permission denials, model refusals and model switches made in the terminal. (#208)
- An MCP form that Claude asks in the terminal now appears in chat as a card telling you to answer it there. (#208)

## Improvements

- Chat approvals now offer the same "always allow" choices as the terminal, including for shell commands. (#208)
- Claude tab labels and busy state on desktop now come from the same session list as Android, so both devices agree. (#207)
- Desktop notifications now say whether a session needs an answer or has finished. On macOS they clear once the session no longer needs you. (#207)
- Switching a desktop Claude pane between Chat and Terminal now only switches the view. It never restarts Claude. (#207)
- Desktop and Android now share the chat composer, approval cards and usage banner, so they behave the same. (#206)
- On Android, resuming a session from project history opens it in your default view, and Restart is now Reconnect: it re-attaches without stopping another device's session. (#206, #207)

## Bug Fixes

- **Security:** fixed an agent-action prompt that starts with `-`, such as `--dangerously-skip-permissions`, being read as a Claude CLI option instead of the prompt. (#207)
- **Security:** fixed desktop Claude terminals starting outside the sandbox runtime when it was enabled but its settings were unavailable. They now refuse to start, as Android-started sessions already did. (#207)
- Fixed `/clear` in the terminal sometimes dropping the next message and reply from chat. `/resume` in the terminal no longer disconnects the chat. (#208)
- Fixed a turn that failed on an API error showing a "success" notice in chat. (#208)
- Fixed an approval card staying answerable after you pressed Esc in the terminal. (#208)
- Fixed files attached in chat while Claude was idle not being read by Claude. (#208)
- Fixed the desktop chat losing an unsent draft and its attachments when you switched tabs or toggled Terminal and Chat. (#206)
- Fixed a desktop pane resuming under your default Claude account after a reload instead of the account it was started with. (#207)
- Fixed Start on startup on Windows when Workbench is installed in a folder with spaces. Also fixed Workbench failing to open if its login item couldn't be set up, and development builds replacing the installed app's login item. (#211)

## Removed

- The server's built-in web page no longer has Spawn buttons or a Running sessions card for `claude remote-control`. Start Claude chats from the desktop or Android app instead. (#205)
