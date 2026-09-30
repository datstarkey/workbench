import { afterEach, describe, expect, it, vi } from 'vitest';
import { paneInput, registerPaneInput, submitPrompt } from './pane-input';

describe('pane input registry', () => {
	afterEach(() => vi.useRealTimers());

	it('pastes the prompt then presses Enter', () => {
		vi.useFakeTimers();
		const calls: string[] = [];
		const off = registerPaneInput('p1', {
			paste: (t) => calls.push(`paste:${t}`),
			key: (d) => calls.push(`key:${JSON.stringify(d)}`)
		});
		expect(submitPrompt('p1', 'line one\nline two')).toBe(true);
		expect(calls).toEqual(['paste:line one\nline two']);
		vi.runAllTimers();
		expect(calls).toEqual(['paste:line one\nline two', 'key:"\\r"']);
		off();
	});

	it('reports a pane with no terminal', () => {
		expect(submitPrompt('missing', 'hi')).toBe(false);
	});

	it('only removes its own registration', () => {
		const a = { paste: vi.fn(), key: vi.fn() };
		const b = { paste: vi.fn(), key: vi.fn() };
		const offA = registerPaneInput('p2', a);
		const offB = registerPaneInput('p2', b);
		offA();
		expect(paneInput('p2')).toBe(b);
		offB();
		expect(paneInput('p2')).toBeUndefined();
	});
});
