# Release v0.27.0

**Released:** 2026-08-14
**Previous version:** v0.26.1

Workbench is now signed with an Apple Developer ID and notarized by Apple. Until now every macOS download arrived as an unidentified developer build — Gatekeeper refused to open it, and getting past that meant right-click → Open or a trip to System Settings. That's gone: the app opens on first launch like any other Mac application.

Signing also fixes notifications properly rather than working around them. macOS refuses to register an unsigned app with its notification service, which is why v0.26.1 shipped a degraded `osascript` fallback (#83). Signed builds use the real notification path, so banners replace each other instead of stacking, and clicking one focuses the session that needs you.

## Improvements

- macOS builds are signed with a Developer ID certificate and notarized by Apple. The app and the disk image are both stapled, so first launch works offline and without Gatekeeper warnings (#85)
- Notifications now use the system notification service on signed builds. Banners replace rather than stack, and clicking a notification focuses the pane that raised it — both previously impossible through the fallback path (#83, #85)
- The minimum supported macOS version is now correctly declared as 10.15. It previously claimed 10.13, which no Tauri v2 build has ever actually supported (#85)

## Upgrade Notes

- **macOS permissions will be requested again.** The application identifier moved to `com.starkeydigital.workbench` as part of signing. macOS keys permission grants to the identifier and signing identity, and both changed, so the system treats this as a new application. Expect fresh prompts for notifications, automation and file access on first launch. This is one-time — future updates keep the same identity.
- Updating from v0.26.x works as normal; the updater replaces the app in place.

## Known Limitations

- Windows installers remain unsigned and still trigger a SmartScreen warning. This needs a separate code signing certificate and is not addressed here.
