# Release v0.45.1

**Released:** 2026-10-07
**Previous version:** v0.45.0

Slash commands work in Claude chats again. Claude Code 2.1.292 stopped plugins from sending a prompt that starts with `/`, so `/compact`, `/code-review` and every other command typed in a chat never ran.

## Bug Fixes

- Fixed slash commands sent from a Claude chat not running. A chat now runs a known command as a command, shows its output, lists the agent a forked skill like `/code-review` starts in the tasks panel, and goes idle when the command is done instead of showing "Thinking" forever. (#226)
- Commands that open a panel in the terminal, such as `/usage` or `/config`, no longer leave the chat stuck. After a few seconds the chat says the command opened in the terminal and offers a **Show terminal** button to switch to it. (#226)

## Development

- `bun run dev` now runs as its own instance with its config in `~/.workbench-dev`, so it can run beside the installed app without changing its projects, settings or Claude plugin. The first run copies your project list. (#226)
