# Release v0.51.0

**Released:** 2026-10-10
**Previous version:** v0.50.0

Claude chats can now interrupt a running turn the way the terminal's Ctrl+Enter does, and show when a conversation is being compacted. The phone's Home shows how many agents and background tasks are still running. A long-running desktop window no longer turns black from terminal graphics memory growing. Workbench also reports freezes with the evidence needed to fix them, and no longer lets slow disk or session work stall the connection the phone and desktop panes use.

## New Features

- Send now in Claude chats: while Claude works, the ⚡ button (or Ctrl+Enter, ⌘↩ on a Mac) stops the current turn and sends your message straight away. A command it was running keeps going in the background; plain Send still adds your message to the running turn (#277)
- Claude chats show a "Compacting conversation" bar with the context size and elapsed time while a conversation is compacted, whether you ran `/compact` or Claude compacted on its own (#277)
- The phone's Home shows running subagents and tasks on each session (e.g. "2 agents · 1 task"), and the total for the machine in the top bar (#275)

## Improvements

- A freeze now reports itself: a stall of a second or more, or the whole app stopping (with the machine's memory pressure at the time), is sent to Sentry, and on macOS every thread's state is saved to `~/.workbench/logs/stall-*.txt` (#276)
- The project list's git status no longer re-syncs on every workspace update, and a folder that becomes a git repository is picked up within 30 seconds (#276)

## Bug Fixes

- Fixed the desktop window going black and freezing after many hours of busy terminals: the terminals' GPU glyph cache no longer grows without limit (#278)
- Fixed the desktop and phone losing their connection for up to minutes while the server waited on slow work: saving workspaces, reading a long chat history, checking the model list's folders and attaching a chat no longer hold up other requests (#276)
- Fixed connections from a client that stopped reading (a phone in the background) being kept open forever, which could exhaust the server's connections (#276)
- Fixed a chat's connection dropping while a session restarted, rewound or switched mode (#276)
