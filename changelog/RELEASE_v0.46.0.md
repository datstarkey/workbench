# Release v0.46.0

**Released:** 2026-10-09
**Previous version:** v0.45.3

Workbench no longer freezes for minutes at a time. A stack sample of a live freeze traced it to the app forking itself to run `git` and `gh`, which stalls every thread in a process this size. Git and GitHub status are now read in-process and from the GitHub API. Terminals start without forking, and no slow request can block the desktop window or the built-in server. The phone's home screen now updates live, and `/goal` works in chats, with a banner for the active goal.

## New Features

- Added a Goal banner to Claude chats. `/goal <condition>` shows the condition and the latest "not met yet" reason above the composer on desktop and phone, then a "Goal achieved" notice when it's done (#245)
- The phone's home screen now updates live over a server stream instead of polling every 4 seconds, and falls back to polling on older desktop hosts (#241)

## Improvements

- Git status, branches, worktrees, log and stashes are now read in-process, typically 4–60× faster than running `git`. Polled `git status` no longer rewrites the index, so it stops setting off refreshes and "index.lock exists" errors for Claude's own git (#240)
- GitHub PR, check and workflow status now comes from the GitHub API using your existing `gh` login, with cached workflow runs that don't count against the rate limit. GitHub Enterprise is supported, and the `gh` CLI is the fallback (#238)
- Long streamed chat replies send about 20× less data to each connected device, and stopping several chats at once is about 3× faster (#247)

## Bug Fixes

- Fixed Workbench freezing completely, desktop window and phone connection, for several minutes, often after sending a message or opening a chat from the phone. Running `git`, `gh`, `claude`, terminals and links no longer forks the app (#235, #244)
- Fixed slow git, GitHub, file or terminal work blocking the desktop window or the built-in server. Blocking work now runs on its own threads, and the server has its own runtime (#236, #237, #242)
- Fixed a large paste into a busy terminal, or heavy terminal output, freezing the server or the window (#242)
- Fixed requests that could hang forever: every `git`, `gh`, GitHub and Trello call now has a timeout, and so do the client requests on desktop and phone (#238, #239, #240, #243)
- Fixed a chat that lost its connection starting a new `claude` every half minute. Reconnects now back off and only re-attach; Restart starts a new one (#239)
- Fixed a chat freezing or dropping messages when one of its requests to Workbench got lost. Every message between Claude and Workbench is now acknowledged, so a lost one is sent again and never applied twice (#248)
- Fixed `/goal`, `/goal clear` and `/rename` leaving a chat's prompt greyed out (#245)
- Fixed a chat's "waiting for approval" state, and its cache keep-warm, staying stuck after the tool had already run or the turn had ended (#246)
- Fixed the sandbox settings file sometimes being written half-finished or with an old allowlist, which could launch Claude without the sandbox (#237)

## Security

- Fixed any process in a chat terminal being able to take over another chat's session or attach under any session id. Terminal tokens are now tied to their own session, and the plugin endpoints only answer on the desktop's loopback listener (#246)
- The hook bridge now requires a per-launch secret, so other local processes can't fake hook events (#248)
- Workbench's per-terminal plugin credentials are no longer inherited when Workbench itself is launched from one of its terminals, and server terminals, Codex chats and the `claude` probes drop any inherited ones (#246)
- Attachment file names are sanitised, so a crafted name can't add files to the "Attached files" list Claude reads (#246)
- A bare shell name is now looked up on `PATH` only, so a cloned repo can't plant its own `bash` to run as your shell (#244)
