# Release v0.49.0

**Released:** 2026-10-09
**Previous version:** v0.48.1

Sessions now work the same way whether you start, open or close them on the desktop, the phone or through the API: the desktop app owns every workspace, tab and session, and every device sends it the same commands and shows the same live state. Closing a session on the phone closes it on the desktop, a session started on the phone opens as a desktop tab, and duplicate `claude` processes are gone. The server also no longer stops answering when many sessions are open.

## New Features

- One session lifecycle for every device. The server keeps your workspaces, tabs and sessions and pushes every change live to the desktop and the phone, which both send the same commands: start, resume, restart, close and rename all run the same code wherever you trigger them. Starting a session on the phone opens a tab on the desktop, ending it on either closes it everywhere, and tabs you haven't looked at keep running. Your existing tabs carry over on the first launch (#262, #268, #270, #271)
- The phone's home screen lists the desktop's workspaces and sessions with their live status, busy time, running tool and how long a session has waited on you; starting shows progress at once, and older desktops keep working (#270)
- Switching the Claude account on the phone switches it on the desktop too (#267)
- Renaming or moving a project updates its open workspaces (#271)

## Improvements

- Native macOS terminals now run on the same terminal engine as every other pane: they respect the project allowlist and permission mode, show on the phone, and end their session when closed (#268)
- Codex terminals launch from your saved settings on every device, report busy and idle from the server, and phone-started Codex sessions now notify you (#268)
- A worktree created from the phone now follows your worktree settings (folder layout, start point, fetch) (#267)
- One update installer for the desktop and the phone: it installs exactly the version you reviewed, shows progress for updates started from the phone, and says which device started an update (#265)
- Workbench comes back in front after an update, even when it was started at login (#269)
- If saved workspaces can't be read, or another Workbench server is using the same settings folder, a banner says so and nothing is overwritten (#271)
- Error reports from the desktop and phone now include source maps, so crashes can be traced to the exact line (#261)
- A log file at `~/.workbench/logs/workbench.log` keeps diagnostics across restarts (#264)
- The Workbench Claude Code plugin now comes only from Workbench itself, always matching your version; an older copy installed from the plugin marketplace is removed automatically (#266)

## Bug Fixes

- Fixed the server API becoming unresponsive with many sessions open: Workbench now raises its open-file limit, and connections from a phone that went to sleep or changed network are closed instead of piling up (#264)
- Fixed duplicate terminals and second `claude` processes appearing after reloading or relaunching the desktop (#263)
- Fixed native workspaces reopening as regular terminals after a relaunch (#263)
- Fixed the Windows release check so signed builds publish on their own (#260)

## Security

- Worktrees can only be created or removed inside registered projects, and the host always decides where they go (#267)
- The server now sets the hook connection for every terminal and chat itself, instead of accepting one from the client (#268)
- A Codex prompt can no longer be read as a command-line flag (#268)
