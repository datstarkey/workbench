# Release v0.6.0

**Released:** 2026-02-13
**Previous version:** v0.5.1

This release brings CI status visibility to every branch -- not just those with pull requests -- and adds a configurable worktree location strategy along with a more resilient worktree deletion flow.

## New Features

- **CI status badges for all branches in the sidebar** -- Previously, GitHub Actions status was only visible for branches that had an open pull request. Now, every branch with recent workflow runs shows a CI status badge directly in the project sidebar. Clicking the badge opens the GitHub Actions sidebar with a detailed breakdown of each workflow run. This is especially useful for branches you are still working on before opening a PR -- you can see at a glance whether your latest push passed CI.

- **GitHub Actions sidebar now shows workflow runs for branches without PRs** -- The GitHub Actions sidebar previously only displayed PR check details. It now also shows workflow run results for any branch, including run name, status, duration, and a link to view the branch on GitHub. Workflow data is batched into a single request per project and pre-fetched during the background poll cycle, so the sidebar opens instantly without a loading delay.

- **Worktree location strategy setting** -- A new setting in Workbench preferences lets you choose where git worktrees are created on disk. The "Sibling folder" strategy (the default, and previous behavior) creates worktrees next to the project folder as `<parent>/<repo>-<branch>`. The new "Inside .worktrees/" strategy creates them at `<repo>/.worktrees/<branch>` and automatically adds `.worktrees/` to your `.gitignore`. This keeps your filesystem tidier if you prefer worktrees grouped inside the repository.

## Bug Fixes

- **Fixed worktree deletion silently failing on dirty worktrees** -- Attempting to delete a worktree that had uncommitted changes would fail with no visible feedback. The confirmation dialog now displays the error message and offers a "Force Remove" button that lets you retry with force deletion, so you can make an informed choice about discarding uncommitted work.
