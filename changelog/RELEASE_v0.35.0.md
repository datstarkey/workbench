# Release v0.35.0

**Released:** 2026-10-02
**Previous version:** v0.34.0

This release tidies up the desktop chat. The header now shows the branch you're on and its pull request status, skills and JSON output display cleanly, and the context meter uses the real window size for the model you're running.

## New Features

- **Branch and PR status in the chat header.** The desktop chat now shows the pane's branch next to the model, plus the PR number, its open/draft/merged/closed state and CI status when the branch has one. Click the PR to open it on GitHub. (#144)

## Improvements

- **Skills stay out of the way.** When Claude loads a skill, its instructions appear inside the collapsed Skill card, rendered as formatted text, not as a long message in the conversation. Other text the CLI adds behind the scenes (reminders, messages from other sessions) is hidden too, matching how saved history already looked. (#143)
- **Readable JSON and agent output.** Tool output that is JSON now shows as a code block, exactly as returned. Subagent reports are rendered as formatted text. (#143)

## Bug Fixes

- Fixed the context meter assuming a 200K or 1M window; it now uses the context window Claude reports for the session's model. (#142)
- Fixed a slash command with extra spaces (such as one picked from the `/` menu) staying stuck at the bottom of the chat after Claude had already received it. (#143)
