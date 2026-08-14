# Release v0.27.1

**Released:** 2026-08-14
**Previous version:** v0.27.0

A fix-only release covering three long-standing failures, each of which had been failing silently. macOS notifications now actually arrive — v0.27.0 said signing had fixed them, which was wrong; signing was necessary but the delivery path was the real cause, and that is fixed here. Windows loses the blank console windows that flashed every few seconds and can open terminals again. Clicking a hyperlink in a terminal now works.

## Bug Fixes

- macOS notifications are delivered again, and for the first time actually work. Workbench posted them through an API Apple retired in macOS 11, and the delivery error was discarded before anything could report it — so every notification vanished without a trace. Notifications now go through the system's current notification service: banners replace each other rather than stacking, and clicking one brings you to the session that raised it (#86, #83)
- Windows no longer flashes blank console windows. Release builds own no console, so every background `git` and `gh` call — on a polling loop — made Windows open a visible one. All child processes now spawn without a console (#87)
- Terminals open again on Windows. The embedded server assumed a Unix shell: it read `$SHELL`, which Windows does not set, fell back to `/bin/bash`, and passed a Unix-only login flag. The spawn failed and surfaced as a server error (#87)
- Fixed a crash when text containing accented characters, dashes or emoji was truncated. Cutting a string at a fixed byte offset can land mid-character, which panics — this took down the hook bridge thread whenever a command contained an em dash at the wrong position (#87)
- Clicking a hyperlink in a terminal now opens it. The click was rejected by a permission check for a dialog that does not exist, failing invisibly with nothing shown and no error surfaced (#88)

## Notes

- The headless server's fallback shell is now `/bin/sh` rather than `/bin/zsh` off macOS. It applies only when `$SHELL` is unset, which is routine under systemd or in slim containers that ship neither zsh nor bash.
