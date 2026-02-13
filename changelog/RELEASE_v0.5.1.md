# Release v0.5.1

**Released:** 2026-02-13
**Previous version:** v0.5.0

A patch release that fixes an issue where newly created GitHub pull requests were not automatically discovered by the GitHub Actions sidebar until a manual refresh occurred.

## Bug Fixes

- **Fixed newly created pull requests not appearing in the GitHub sidebar automatically** -- The GitHub integration's background polling was only checking projects that already had known open PRs. This meant that when you pushed a new branch and opened a pull request for the first time, the sidebar would not detect it until you navigated away and back or triggered a manual refresh. The polling loop now checks all projects with visible branches, so new PRs are picked up within the next poll cycle (up to 90 seconds) without any manual intervention.
