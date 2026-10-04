# Release v0.37.0

**Released:** 2026-10-04
**Previous version:** v0.36.0

Workbench now tracks Claude sessions through its own Claude Code plugin instead of a hook script in your Claude settings. Nothing is written to `~/.claude/settings.json` any more, and the plugin is also installable from this repo. Skills Claude loads mid-chat now stay folded into their card.

## New Features

- **Workbench Claude Code plugin.** Session activity (started, working, waiting, done) and the git refresh after git/gh commands now come from a Claude Code plugin that Workbench loads into every Claude terminal and chat it starts. It needs no setup and doesn't touch your Claude settings. To keep it in sessions started outside Workbench, run `/plugin marketplace add datstarkey/workbench` then `/plugin install workbench@workbench`; it only reports when Workbench started the session. Requires a Claude Code version with plugin function hooks (tested on 2.1.286). (#155)

## Improvements

- **No more Claude hook script.** On launch, Workbench removes the `workbench-hook-bridge` script and its entries from every Claude account's `settings.json`, and the "Session hooks" setting and the Claude integration prompt are gone. Codex keeps its notify script. (#155)
- **Git refresh after file edits.** The sidebar's git state now also refreshes after Claude's Write and Edit tools, not only after git/gh shell commands. (#155)

## Bug Fixes

- Fixed a skill Claude loaded during a chat sometimes showing its whole text as a message instead of inside the collapsed Skill card. (#156)
