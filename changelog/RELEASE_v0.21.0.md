# Release v0.21.0

**Released:** 2026-06-02
**Previous version:** v0.20.0

This release graduates Settings from an in-app floating panel to a real, separate desktop window — drag it by its native title bar, move it to another monitor, and your changes apply back to the main window instantly. It also tidies up a small alignment glitch across the three main columns.

## New Features

- **Settings now opens in its own OS window.** Instead of a floating panel pinned inside the main window, Settings is a proper desktop window with a native title bar — so it's easy to grab, move anywhere (including a second display), and resize. Its position and size are still remembered between sessions, and changes you save (accent, sidebar toggles, integrations) update the main window right away (#68)

## Improvements

- Aligned the workspace tab strip with the Projects and Git/GitHub sidebar headers so the divider lines across the three columns line up cleanly (#67)
