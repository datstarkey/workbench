# Release v0.44.0

**Released:** 2026-10-07
**Previous version:** v0.43.1

Phone notifications work again, and the desktop sidebar gets the favourites the phone already had.

## New Features

- Added favourite projects to the desktop sidebar. Star a project from its hover star or the project menu and it moves into a Favourites section at the top, as on the phone. Favourites are kept per machine, follow a project when its path changes, and when favourites or groups exist the remaining projects sit under an "Other" header. (#221)

## Bug Fixes

- Fixed Android notifications never arriving. The phone treated Workbench as always on screen, so every "Turn complete" and approval alert was dropped. Alerts now post whenever the app is in the background. (#222)
- Fixed notifications stopping while the phone is locked. Turning notifications on now asks to let Workbench run in the background; allow it, or Android's battery saving cuts the app's connection to your computer. (#222)
- Fixed tapping a notification opening only the app instead of its conversation. (#222)
- Fixed the project filter matching every project whose path contained the search word. It now matches paths only when the search contains a `/`. (#221)
