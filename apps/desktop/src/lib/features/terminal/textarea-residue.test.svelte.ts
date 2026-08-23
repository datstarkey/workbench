import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { installTextareaResidueGuard } from './textarea-residue';

describe('installTextareaResidueGuard', () => {
	let textarea: HTMLTextAreaElement;
	let uninstall: () => void;

	beforeEach(() => {
		vi.useFakeTimers();
		textarea = document.createElement('textarea');
		uninstall = installTextareaResidueGuard(textarea);
	});

	afterEach(() => {
		uninstall();
		vi.useRealTimers();
	});

	function insert(text: string) {
		textarea.value += text;
		textarea.dispatchEvent(new Event('input'));
	}

	it('clears the textarea after each keystroke on the next macrotask', () => {
		insert('H');
		expect(textarea.value).toBe('H');
		vi.runAllTimers();
		expect(textarea.value).toBe('');
	});

	it('never lets residue accumulate across a typed run', () => {
		for (const ch of 'H W THIS') {
			insert(ch);
			vi.runAllTimers();
		}
		expect(textarea.value).toBe('');
	});

	it('leaves the textarea alone during an IME composition', () => {
		textarea.dispatchEvent(new Event('compositionstart'));
		insert('に');
		insert('にほ');
		vi.runAllTimers();
		expect(textarea.value).toBe('ににほ');
	});

	it('clears after the composition ends, on a later tick than xterm reads', () => {
		textarea.dispatchEvent(new Event('compositionstart'));
		insert('日本');
		textarea.dispatchEvent(new Event('compositionend'));
		const xtermRead = vi.fn(() => textarea.value);
		setTimeout(xtermRead, 0);
		insert('');
		vi.runAllTimers();
		expect(xtermRead).toHaveReturnedWith('日本');
		expect(textarea.value).toBe('');
	});

	it('stops clearing once uninstalled', () => {
		uninstall();
		insert('A');
		vi.runAllTimers();
		expect(textarea.value).toBe('A');
		uninstall = () => {};
	});
});
