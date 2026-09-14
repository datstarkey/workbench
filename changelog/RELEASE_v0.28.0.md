# Release v0.28.0

**Released:** 2026-09-14
**Previous version:** v0.27.2

This release is about running Claude with fewer prompts without giving up the safety net. Claude Code v2.1.257 stopped honouring `bypassPermissions` and `auto` from a project's `.claude/settings.json`, so Workbench now owns the permission mode and passes it on the command line. Alongside it, a new opt-in sandbox runtime confines the whole Claude process (file tools, MCP servers and hooks, not only Bash), which is what makes bypass mode reasonable to use. Windows users also get a fix for startup commands that never ran.

## Breaking Changes

- Removed the Happy Coder integration. The "Happy Coder" toggle is gone and Claude sessions always launch the `claude` CLI. Existing settings files that still contain the old `useHappyCoder` key load unchanged and the key is dropped on the next save (#93)

## New Features

- Added a **Claude permission mode** setting (Settings → Workbench). Choose Default, Accept Edits, Plan, Don't Ask, Auto or Bypass Permissions and Workbench appends `--permission-mode <mode>` to every new and resumed Claude session. This restores per-mode launches after Claude Code v2.1.257 stopped reading `bypassPermissions` and `auto` from project-scope settings. Changing the mode updates the saved startup command of every open Claude pane, including ones started with an initial prompt. Choosing Bypass shows a warning that it skips every permission check (#93)
- Added an opt-in **sandbox runtime** for Claude sessions (Settings → Workbench, macOS and Linux). When on, every Claude launch and resume is wrapped in Anthropic's `@anthropic-ai/sandbox-runtime`, so the whole `claude` process runs behind the operating-system sandbox: file tools, MCP servers and hooks included. Sandboxed sessions can write to your project folders, `/tmp` and Claude Code's own session data, and nothing else. Hooks, skills, agents, plugins, MCP server config, git hooks and Workbench's own settings are read-only, so a session cannot install code that would run unsandboxed later. Credential directories such as `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.kube` and `~/.docker` are unreadable. Network access is limited to Claude Code's required hosts plus a domain allowlist you control. Needs Node (`npx`); Linux also needs `bubblewrap`, `socat` and `ripgrep`. The first launch downloads the runtime from npm (#94)

## Improvements

- Server mode and the sandbox runtime cannot be on at the same time. A sandboxed session could otherwise reach the local control-plane server and spawn an unsandboxed process, so starting the server is refused while the sandbox is enabled, and enabling the sandbox stops a running server (#94)
- Project startup commands and agent tasks that launch `claude` now honour the permission mode and sandbox settings too. A permission flag you wrote yourself in a startup command is left as is (#94)

## Bug Fixes

- Windows: startup commands now run in the shell panes actually use. The v0.27.1 fix landed in a code path Windows never hits, and the command was also sent before the console shell had finished initialising, so it was silently swallowed. Commands are now submitted once the shell is ready, with the correct line ending (#92)
