import { afterEach, describe, expect, it, vi } from 'vitest';
import { touchScroll } from './touch-scroll.ts';

// jsdom has no Touch constructor, so build plain events carrying a `touches` list.
function touch(target: Element, type: string, ys: number[]) {
	const ev = new Event(type, { bubbles: true, cancelable: true });
	Object.defineProperty(ev, 'touches', { value: ys.map((clientY) => ({ clientX: 5, clientY })) });
	target.dispatchEvent(ev);
	return ev;
}

describe('touchScroll', () => {
	const host = document.createElement('div');
	const xterm = host.appendChild(document.createElement('div'));
	xterm.className = 'xterm';
	const screen = xterm.appendChild(document.createElement('div'));
	screen.className = 'xterm-screen';
	const wheel = vi.fn((e: WheelEvent) => ({ deltaY: e.deltaY, target: e.target }));
	host.addEventListener('wheel', wheel);
	let dispose = touchScroll(host);
	const deltas = () => wheel.mock.results.map((r) => r.value.deltaY);

	afterEach(() => {
		wheel.mockClear();
		dispose();
		dispose = touchScroll(host);
	});

	it('leaves small movements alone so a tap still focuses the terminal', () => {
		touch(screen, 'touchstart', [100]);
		const move = touch(screen, 'touchmove', [95]);

		expect(move.defaultPrevented).toBe(false);
		expect(wheel).not.toHaveBeenCalled();
	});

	it('replays a drag past the slop as line-sized wheel events on the touched element', () => {
		touch(screen, 'touchstart', [100]);
		touch(screen, 'touchmove', [88]);
		const move = touch(screen, 'touchmove', [40]);

		expect(move.defaultPrevented).toBe(true);
		expect(deltas()).toEqual([16, 16, 16]);
		expect(wheel.mock.results[0].value.target).toBe(screen);
	});

	it('carries sub-step remainders and handles direction changes', () => {
		touch(screen, 'touchstart', [100]);
		touch(screen, 'touchmove', [80]);
		touch(screen, 'touchmove', [70]);
		touch(screen, 'touchmove', [120]);

		expect(deltas()).toEqual([-16, -16]);
	});

	it('retargets drags that start on the host padding to the xterm screen', () => {
		touch(host, 'touchstart', [100]);
		touch(host, 'touchmove', [85]);
		touch(host, 'touchmove', [60]);

		expect(wheel.mock.results[0].value.target).toBe(screen);
	});

	it('ignores multi-finger gestures and moves after touchend', () => {
		touch(screen, 'touchstart', [100, 200]);
		touch(screen, 'touchmove', [20, 150]);
		touch(screen, 'touchstart', [100]);
		touch(screen, 'touchend', []);
		const move = touch(screen, 'touchmove', [20]);

		expect(wheel).not.toHaveBeenCalled();
		expect(move.defaultPrevented).toBe(false);
	});

	it('stops listening once disposed', () => {
		dispose();
		touch(screen, 'touchstart', [100]);
		touch(screen, 'touchmove', [20]);

		expect(wheel).not.toHaveBeenCalled();
	});
});
