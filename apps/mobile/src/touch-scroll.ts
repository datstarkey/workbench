// xterm.js 6 has no working touch scrolling: its Gesture handler ships in the
// bundle but is never attached to the viewport, so a finger drag does nothing.
// Replay one-finger vertical drags as wheel events on the terminal — xterm's own
// wheel path then scrolls the scrollback, or sends arrows / mouse reports when a
// full-screen app (vim, less, a TUI) owns the screen.

// Movement below this stays a tap: cancelling the first touchmove suppresses the
// synthesized mousedown xterm focuses its textarea (and the keyboard) on.
const SLOP_PX = 10;
// xterm emits at most one arrow / mouse report per wheel event in full-screen
// apps, so big swipes are split into roughly line-sized steps.
const STEP_PX = 16;

export function touchScroll(el: HTMLElement): () => void {
	let startY: number | undefined;
	let lastY = 0;
	let dragging = false;
	let pending = 0;

	const reset = () => {
		startY = undefined;
		dragging = false;
		pending = 0;
	};
	const onStart = (e: TouchEvent) => {
		reset();
		if (e.touches.length === 1) startY = lastY = e.touches[0].clientY;
	};
	const onMove = (e: TouchEvent) => {
		if (startY === undefined || e.touches.length !== 1) return reset();
		const { clientX, clientY } = e.touches[0];
		if (!dragging) {
			if (Math.abs(startY - clientY) < SLOP_PX) return;
			dragging = true;
			lastY = clientY;
		}
		e.preventDefault();
		pending += lastY - clientY;
		lastY = clientY;

		// A drag starting on the host's padding targets the host, which is above
		// xterm's wheel listener — aim at the screen instead.
		const target =
			e.target instanceof Element && e.target.closest('.xterm')
				? e.target
				: (el.querySelector('.xterm-screen') ?? el);
		while (Math.abs(pending) >= STEP_PX) {
			const deltaY = Math.sign(pending) * STEP_PX;
			pending -= deltaY;
			target.dispatchEvent(
				new WheelEvent('wheel', { deltaY, clientX, clientY, bubbles: true, cancelable: true })
			);
		}
	};

	el.addEventListener('touchstart', onStart, { passive: true });
	el.addEventListener('touchmove', onMove, { passive: false });
	el.addEventListener('touchend', reset);
	el.addEventListener('touchcancel', reset);
	return () => {
		el.removeEventListener('touchstart', onStart);
		el.removeEventListener('touchmove', onMove);
		el.removeEventListener('touchend', reset);
		el.removeEventListener('touchcancel', reset);
	};
}
