# Release v0.40.0

**Released:** 2026-10-06
**Previous version:** v0.39.1

Notifications now come from one place, so a Claude session that needs you alerts on both the desktop and the phone, whichever device started it. The agents panel shows what each agent is doing as a conversation and can be resized. Claude's plan shows up as a checklist again. The phone's home screen gets a new project list with favourites, groups and search.

## New Features

- **One alert source for desktop and phone.** Approvals and finished turns now alert on both devices, for sessions started on either one. Desktop terminals waiting on an approval now reach the phone (marked "answer in its terminal"). Phone-started sessions now notify the desktop. Native terminal Claude panes now show on the phone. Keep-warm turns no longer count as finished. (#181)
- **See each agent's conversation.** Pick an agent in the agents panel to read its whole conversation as it runs, or switch to its raw output. (#180)
- **Resizable agents panel.** Drag its edge, or use the arrow keys, to set its width on desktop. Workbench remembers it. (#180)
- **Plan checklist is back.** Claude Code's current planning tools (TaskCreate, TaskUpdate, TaskList) show as the checklist above the composer, with the step in progress named. (#179)
- **New phone project list.** Star projects to pin them, browse your desktop's groups as collapsible sections, and search by name, path or group. The match is highlighted and a clear button resets the search. (#177)

## Bug Fixes

- Fixed "Always allow" missing from approvals in chat, which made tools like Claude in Chrome ask on every call. It now allows that tool for the rest of the session, as the terminal does. (#178)
- Fixed the chat's mode picker showing your settings default (such as Auto) instead of the session's real mode (such as Bypass). (#176)
- Fixed slash commands only being offered at the start of the message. They now work mid-line too, and pressing Enter there sends the message instead of picking a command. (#174, #175)
- Fixed parallel agents being matched to the wrong task in the agents panel. (#173)

## Notes

- Most of these fixes are in the Workbench plugin that Claude sessions run. Sessions already running keep the old plugin until they restart.
