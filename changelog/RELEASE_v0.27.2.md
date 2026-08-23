# Release v0.27.2

**Released:** 2026-08-23
**Previous version:** v0.27.1

A fix-only release. The long-standing terminal bug where typing capital letters made a chunk of text reappear with a run of extra spaces is gone for good — the earlier guard only caught a fraction of cases, and this release fixes the underlying cause. Windows users get session history, hooks and resumed sessions working again.

## Bug Fixes

- Typing in a terminal no longer repeats capitalised text with extra spaces. On macOS, WebKit inserts capital letters and spaces into the terminal's hidden input buffer in a way lowercase letters never are, and a stray composition event then replayed that buffer as typed input — which is why the echo was always capitals and spaces only. The buffer is now kept empty between keystrokes, so there is nothing to replay. The earlier heuristic from v0.27.0 stays in place as a safety net, but should no longer fire (#90)
- Windows: Claude session history and resume work again. Project paths were encoded differently from how the Claude CLI names its session folders, so discovery always found nothing (#89)
- Windows: Claude Code hooks register and run. The hook command path was stripped of its backslashes by the shell Claude Code uses, so PowerShell rejected every event. Paths are now quoted and normalised, and stale broken registrations are cleaned up (#89)
- Windows: resumed sessions and startup commands actually execute instead of sitting unsubmitted at the prompt (#89)
