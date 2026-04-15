/**
 * Module-level layout suppression flag.
 *
 * VS Code wraps structural DOM changes (add/remove split panes) in
 * `_withDisabledLayout()` to prevent ResizeObserver callbacks from
 * triggering resize storms with intermediate dimensions.
 *
 * This module provides the same pattern for our terminal panes.
 */

let _disabled = false;

export function isLayoutDisabled(): boolean {
	return _disabled;
}

export function suppressLayout(fn: () => void) {
	_disabled = true;
	fn();
	// setTimeout(0) fires in the next macrotask, which is after paint.
	// ResizeObserver callbacks fire between layout and paint.
	// This ensures the flag stays true during the intermediate-dimension
	// ResizeObserver callbacks triggered by the DOM mutation.
	setTimeout(() => {
		_disabled = false;
	}, 0);
}
