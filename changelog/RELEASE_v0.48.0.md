# Release v0.48.0

**Released:** 2026-10-09
**Previous version:** v0.47.0

Claude sessions now get titles that describe the work instead of the first words of your prompt, Claude accounts (the default one included) can be renamed, and a chat no longer freezes on a stale tool, turn or agents panel until you switch to the terminal and back.

## New Features

- Claude sessions now name themselves from the conversation: after the first few prompts, then every fifth, a short title is generated with Haiku (on the session's own login) and shown in the chat and sidebar. A name you give with `/rename <name>` is kept and stops automatic titles for that conversation, across restarts (#255)
- Added Rename account to the Claude account switcher, for every account including the default `~/.claude` login. The new names show on desktop and phone (#254)

## Improvements

- The `/` command menu in chat now matches a plugin command by its own name and by each word, so `/a` or `/sign` finds `/starkeydigital:app-signing` near the top. A built-in you type in full still comes first (#253)

## Bug Fixes

- Fixed chats that sometimes stopped updating, leaving a tool running, a turn unfinished or a forked skill's agents panel closed until you switched to Terminal and back. A duplicate command name in the `/` menu crashed the chat's rendering (#252)
