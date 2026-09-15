# Release v0.29.1

**Released:** 2026-09-15
**Previous version:** v0.29.0

A fix-only release for the new cross-device terminals.

## Bug Fixes

- The desktop **Take control** button, shown when a terminal is open on another device, can now be clicked. xterm's WebGL renderer draws a transparent canvas over the terminal that sat on top of the overlay, so the button was visible but every click went to the terminal instead. The same applied to the **Dismiss** button on terminal error overlays (#105)
