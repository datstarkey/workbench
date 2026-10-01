# Release v0.31.0

**Released:** 2026-10-01
**Previous version:** v0.30.0

A chat release. Claude panes can now switch from the terminal to a native chat view, on the desktop and on the phone, where a new chat-first home shows what needs your attention. You can also run several Claude logins side by side and see each one's plan usage. The settings window is reorganised, tabs can be dragged and split, and several Windows problems are fixed.

## New Features

- **Claude chat view.** Every Claude pane has a Terminal | Chat switch. Chat shows the same session as messages instead of a TUI. (#117)
  - Replies stream in as formatted text. Tool calls show as cards with diffs, live timers and the full output on request.
  - Approvals, questions and plans appear as cards you answer in place. A chat in a hidden tab still flags the tab and sends a notification when it needs you.
  - Paste, pick or drop images into a prompt. A `/` menu lists the CLI's commands, with pickers for `/resume`, the model, effort and permission mode.
  - A panel lists subagents and background tasks, with live output for background shells.
  - An activity line shows whether Claude is thinking, writing, running a tool or retrying. A banner shows when you hit a usage limit, and a meter shows how full the context is.
  - Settings can make new Claude tabs open as chat. Chat is not available while the sandbox runtime is on.
- **Chat on the phone.** (#124)
  - A new home screen has three lists. **Needs you** holds waiting approvals, which you can allow or deny right there. **Running** holds live chats and terminals from any device. **Start** opens a chat or terminal in any project or worktree.
  - Every conversation has a Chat | Terminal switch. Approvals and questions rise as a sheet you can lower to read the chat behind it.
  - New Claude sessions open as chat; Settings can make the terminal the default. The extra-keys row gains Shift+Tab.
  - The desktop app must run this version for the phone's chat to work.
- **Multiple Claude accounts.** Run several Claude logins side by side, for example work and personal, and switch between them from the status bar. Each account has its own config folder. New sessions use the active account, a resumed session uses the account it was created under, and running sessions are never moved. (#120)
- **Plan usage per account.** The account menu shows how much of each limit every logged-in account has used, for example `Session 3% · Week 89%`, in the warning colour from 80%. The tooltip lists every limit with its reset time. (#121)
- **Drag to reorder tabs.** Project tabs and terminal/chat tabs can be dragged into a new order. (#122)
- **Split view.** Split now shows the active tab next to its neighbour, side by side or stacked, instead of adding an empty shell. Click another tab to show it alone; click either half to bring the split back. Sessions keep running either way. (#122)
- **Codex launch settings.** A new Codex › Sessions page sets the approval policy and sandbox mode for new Codex sessions. "Codex default" leaves `~/.codex/config.toml` in charge. (#125)

## Improvements

- **Redesigned settings window.** The long General page is split into General, Worktrees, Terminal, Agent actions, Integrations and Remote access. Claude launch settings move to Claude Code › Sessions, and every page uses one consistent label-and-control layout. (#125)
- **Ctrl+,** opens Settings on Windows and Linux. The version label in the status bar is now a button that checks for updates. (#129)
- Long terminal tab names are shortened, with the full name shown on hover. (#122)
- The phone's terminal reconnects by itself after the phone sleeps, instead of staying closed. (#123)

## Bug Fixes

- Codex terminals can now be scrolled on desktop and phone. Codex runs inline, so mouse-wheel and touch scrolling no longer turn into history navigation. (#119)
- Updating the app no longer leaves the old version's sessions running: a second Dock icon on macOS, or a stray console window on Windows. (#118)
- GitHub and Trello links open on Windows. They used to fail with "Windows cannot find '\'". (#126)
- The light native menu bar no longer appears under the dark title bar on Windows and Linux. (#129)
- In chat, prompts sent mid-turn with an image, or sent around `/clear`, no longer stay stuck as pending. (#128)
