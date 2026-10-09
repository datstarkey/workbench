# Release v0.48.1

**Released:** 2026-10-09
**Previous version:** v0.48.0

The Windows installer is now code-signed, so SmartScreen no longer warns about an unknown publisher. A question that Claude asks while no chat is open now waits in the terminal instead of showing as withdrawn. Workbench has moved to the `starkey-digital` GitHub organization.

## Improvements

- Signed the Windows app and installer as Starkey Digital Ltd, so Windows no longer shows the "unknown publisher" SmartScreen warning when you install or update (#257)
- Moved the repository to [starkey-digital/workbench](https://github.com/starkey-digital/workbench). Desktop updates continue as before. Android installs older than this release need the new APK installed by hand once, because their updater only accepts the old download address (#257)

## Bug Fixes

- Fixed questions and approvals that said "Question withdrawn by Claude" in chat when Claude asked them while no chat was open, even though the terminal was still waiting for an answer. The chat now shows "Waiting for your answer in the terminal" with a Show terminal button, then your answers once you reply there. The session stays marked as waiting, and the phone no longer offers Allow/Deny buttons that could not work (#258)
