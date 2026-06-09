import { describe, it, expect, vi, beforeEach } from 'vitest';

const captureMessage = vi.fn();
const withScope = vi.fn((cb: (scope: unknown) => void) => {
	cb({
		setLevel: vi.fn(),
		setTag: vi.fn(),
		setContext: vi.fn()
	});
});

vi.mock('@sentry/svelte', () => ({
	captureMessage: (...args: unknown[]) => captureMessage(...args),
	withScope: (cb: (scope: unknown) => void) => withScope(cb)
}));

import { TerminalInputDedup } from './input-dedup';

/** Type a run of single characters, each recorded as its own onData emit. */
function typeRun(dedup: TerminalInputDedup, text: string, startAt: number, stepMs = 50): number {
	let t = startAt;
	for (const ch of text) {
		// Each keystroke first passes the duplicate check (never a dup), then records.
		expect(dedup.isDuplicateFlush(ch, t)).toBe(false);
		dedup.recordSent(ch, t);
		t += stepMs;
	}
	return t;
}

describe('TerminalInputDedup', () => {
	beforeEach(() => {
		captureMessage.mockClear();
		withScope.mockClear();
		vi.spyOn(console, 'warn').mockImplementation(() => {});
	});

	function make(): TerminalInputDedup {
		return new TerminalInputDedup('session-1234abcd', () => 'shell');
	}

	it('drops a multi-char blob that reproduces a just-typed run', () => {
		const dedup = make();
		const t = typeRun(dedup, 'hello world', 1000);
		// xterm re-emits the whole run as one blob (the bug).
		expect(dedup.isDuplicateFlush('hello world', t)).toBe(true);
	});

	it('drops a blob that is a contiguous subset of the recent run', () => {
		const dedup = make();
		const t = typeRun(dedup, 'twping twice if ', 1000);
		expect(dedup.isDuplicateFlush('twice if ', t)).toBe(true);
	});

	it('passes single characters through (length < 2)', () => {
		const dedup = make();
		typeRun(dedup, 'abc', 1000);
		expect(dedup.isDuplicateFlush('a', 1200)).toBe(false);
	});

	it('passes control sequences through (arrows, Enter, bracketed paste)', () => {
		const dedup = make();
		typeRun(dedup, 'ls', 1000);
		expect(dedup.isDuplicateFlush('\x1b[A', 1100)).toBe(false); // up arrow
		expect(dedup.isDuplicateFlush('\x1b\r', 1100)).toBe(false); // claude Shift+Enter
		expect(dedup.isDuplicateFlush('\x1b[200~ls\x1b[201~', 1100)).toBe(false);
	});

	it('does not drop a multi-char blob that was never typed', () => {
		const dedup = make();
		typeRun(dedup, 'abc', 1000);
		expect(dedup.isDuplicateFlush('xyz', 1200)).toBe(false);
	});

	it('stands down for a real paste within the grace window', () => {
		const dedup = make();
		const t = typeRun(dedup, 'hello world', 1000);
		dedup.notePaste(t);
		expect(dedup.isDuplicateFlush('hello world', t + 100)).toBe(false);
		// ...but resumes once the paste grace window passes.
		expect(dedup.isDuplicateFlush('hello world', t + 2000)).toBe(true);
	});

	it('does not match a run that scrolled out of the time window', () => {
		const dedup = make();
		const t = typeRun(dedup, 'hello world', 1000);
		// 5s later the history has been pruned past the 4s window.
		expect(dedup.isDuplicateFlush('hello world', t + 5000)).toBe(false);
	});

	it('reports to Sentry on drop, throttled to one event per window', () => {
		const dedup = make();
		let t = typeRun(dedup, 'abcdef', 1000);
		expect(dedup.isDuplicateFlush('abcdef', t)).toBe(true);
		expect(captureMessage).toHaveBeenCalledTimes(1);
		// A second drop shortly after is suppressed from Sentry (still dropped).
		t = typeRun(dedup, 'abcdef', t + 100);
		expect(dedup.isDuplicateFlush('abcdef', t)).toBe(true);
		expect(captureMessage).toHaveBeenCalledTimes(1);
	});
});
