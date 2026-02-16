# Release v0.7.0

**Released:** 2026-02-16
**Previous version:** v0.6.1

This release introduces Trello board integration, one-click PR merging from the sidebar, configurable agent actions for launching AI sessions with pre-built prompts, and several quality-of-life improvements that make it easier to know when Claude needs your attention. Terminal responsiveness during AI sessions has also been significantly improved.

## New Features

- **Trello board integration** -- You can now connect Trello boards to your projects. Authenticate with your Trello API key and token in Settings, then link a board to any project. A new "Boards" tab in the right sidebar shows your cards organized by list, with inline card creation and the ability to link tasks to branches or worktrees. You can also configure automated actions -- such as moving a card to a different column or toggling labels -- that trigger when you link a task to a branch or when a pull request is merged.

- **Agent Actions: configurable prompt templates for AI sessions** -- A new "Agent Actions" feature lets you define reusable prompt templates that launch Claude or Codex sessions with a pre-filled prompt. Each action has a name, prompt text, target (Claude, Codex, or both), category, and optional tags. Configure your actions in Workbench settings, then trigger them from the Agent Actions dropdown menu to instantly start a focused AI session without retyping common instructions.

- **Merge pull requests directly from the GitHub sidebar** -- Open pull requests now show a Merge button in the GitHub sidebar header. The button performs a squash merge and only appears when the PR is open, not a draft, and all checks have passed. After merging, Workbench automatically refreshes the project status so that any linked Trello automation (like moving a card on merge) fires immediately.

- **Optional branch deletion when removing a worktree** -- The worktree removal confirmation dialog now includes an "Also delete branch" checkbox. When the branch has a merged pull request, the checkbox is pre-selected so you can clean up in a single step. This makes it easy to tidy up after finishing work on a feature branch without having to run separate git commands.

- **Dock badge shows how many sessions need your attention** -- The macOS dock icon now displays a badge count reflecting the number of Claude sessions that are waiting for input. At a glance you can tell whether any session is blocked -- even when Workbench is in the background or minimized.

- **Visual distinction between "awaiting input" and "idle" Claude sessions** -- The sidebar now uses a red alert icon when Claude is blocked waiting for you to approve a permission request or answer a question, clearly distinguishing it from the amber/sky pause icon used for idle sessions. This makes it much easier to spot which sessions actually need your attention versus which ones have simply finished.

- **Integration approval dialog for external config changes** -- When an integration needs to modify files outside of Workbench (such as Claude Code settings files), you are now shown an approval dialog explaining what will be changed before any external configuration is written. This gives you visibility and control over changes that affect tools outside of Workbench.

## Improvements

- **Faster and more reliable Escape key in AI terminal sessions** -- Pressing Escape to stop Claude or Codex generation is now significantly more responsive. Workbench sends a double-escape sequence so the terminal input library can instantly recognize it without waiting through a ~100ms disambiguation timeout. Additionally, a new two-thread output pipeline (inspired by Alacritty and Kitty) coalesces heavy terminal output during streaming, reducing IPC events from roughly 1,000/sec to 250/sec. This frees up the input channel so your Escape keypress is processed immediately rather than queuing behind a flood of output events.

## Bug Fixes

- **Fixed double-Escape killing Claude sessions** -- A race condition in the SIGINT escalation logic meant that pressing Escape twice in quick succession could terminate an entire Claude session rather than just stopping the current generation. The problematic fallback has been removed while preserving the fast double-escape disambiguation introduced in the same release cycle.
