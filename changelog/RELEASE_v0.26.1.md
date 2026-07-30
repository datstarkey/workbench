# Release v0.26.1

**Released:** 2026-07-30
**Previous version:** v0.26.0

A maintenance release focused on GitHub API usage and two long-standing annoyances. Workbench was burning through the GitHub API hourly rate limit within minutes of an active AI session — every file edit triggered a full GitHub refresh. That is fixed, along with AI session tabs that never showed their real name and macOS notifications that never appeared.

## Improvements

- GitHub polling no longer exhausts your API rate limit. Editing files in an AI session used to trigger a full GitHub refresh behind a 300ms debounce — up to ~200 refreshes per minute per project, enough to empty the hourly budget in under ten minutes. Local edits now cost nothing, since they cannot change GitHub state, and the refreshes that remain are throttled to one per project per minute (#82)
- A `git push` at the end of a commit burst still refreshes promptly rather than being throttled away, so pull request and CI state stay current when it matters (#82)
- Pull request check details now come from the pull request list response instead of an extra API call per open pull request on every poll, so API cost no longer grows with the number of open pull requests (#82)
- Background polling intervals relaxed — 30s while CI is running, 3 minutes when idle (#82)
- CI status now resolves for branches that fall outside the repository's recent-run window, so on a busy repository a branch no longer shows no CI status at all (#82)

## Bug Fixes

- AI session tabs now show the session's real name. Every tab previously stayed on its `Session a1b2c3d4` placeholder for its entire life: the name is taken from your first message, but it was looked up once at session start — before that message existed — and the miss was then cached permanently (#84)
- Session names now resolve in git worktrees, which previously looked up sessions against the parent project directory and so never found them (#84)
- macOS notifications now appear when a session needs your input. Workbench is distributed without an Apple Developer ID signature, which macOS requires before it will register an app for notifications — so every notification was silently discarded. Notifications are now posted through a fallback path that works on unsigned builds (#84)
- Fixed the session resume list being replaced while you were looking at it, when activity in one project overwrote the list another project's landing page was showing (#84)

## Known Limitations

- The macOS notification fallback posts under Script Editor's identity, stacks banners rather than replacing them, and cannot focus the relevant pane when clicked. All three resolve once the app ships with a Developer ID signature (#83)
