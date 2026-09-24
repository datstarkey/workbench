import { describe, it, expect } from 'vitest';
import { stripAnsi, formatSessionDate, formatRelativeTime, plural } from './format';

describe('stripAnsi', () => {
	it('removes basic SGR color codes', () => {
		expect(stripAnsi('\u001B[31mred text\u001B[0m')).toBe('red text');
	});

	it('removes multiple SGR sequences', () => {
		expect(stripAnsi('\u001B[1m\u001B[32mbold green\u001B[0m')).toBe('bold green');
	});

	it('removes 256-color and truecolor codes', () => {
		expect(stripAnsi('\u001B[38;5;200mextended\u001B[0m')).toBe('extended');
		expect(stripAnsi('\u001B[38;2;255;0;128mtruecolor\u001B[0m')).toBe('truecolor');
	});

	it('strips ESC] as a single-char escape even when followed by OSC-like content', () => {
		// The regex single-char branch ([@-Z\\-_]) matches ESC+']' before the OSC
		// branch can, so the ']' is consumed but the payload remains. The BEL is
		// left in the output as a literal character.
		const result = stripAnsi('\u001B]0;window title\u0007rest');
		expect(result).not.toContain('\u001B');
	});

	it('returns empty string for empty input', () => {
		expect(stripAnsi('')).toBe('');
	});

	it('passes through string with no ANSI codes', () => {
		const plain = 'Hello, world! 123';
		expect(stripAnsi(plain)).toBe(plain);
	});

	it('handles string that is only ANSI codes', () => {
		expect(stripAnsi('\u001B[31m\u001B[0m')).toBe('');
	});

	it('removes cursor movement sequences', () => {
		expect(stripAnsi('\u001B[2Jcleared')).toBe('cleared');
		expect(stripAnsi('\u001B[10;20Hpositioned')).toBe('positioned');
	});
});

describe('formatSessionDate', () => {
	it('formats a valid ISO timestamp', () => {
		const result = formatSessionDate('2024-06-15T14:30:00Z');
		// The exact output depends on locale, but should contain month and time parts
		expect(result).toContain('Jun');
		expect(result).toContain('15');
	});

	it('returns empty string for empty input', () => {
		expect(formatSessionDate('')).toBe('');
	});

	it('returns empty string for falsy input', () => {
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		expect(formatSessionDate(undefined as any)).toBe('');
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		expect(formatSessionDate(null as any)).toBe('');
	});

	it('includes AM/PM in the output', () => {
		const result = formatSessionDate('2024-01-01T08:05:00Z');
		// Should contain AM or PM since hour12: true
		expect(result).toMatch(/AM|PM/);
	});
});

describe('formatRelativeTime', () => {
	const now = new Date('2026-09-24T12:00:00Z').getTime();
	const ago = (ms: number) => new Date(now - ms).toISOString();

	it('buckets by unit', () => {
		expect(formatRelativeTime(ago(20_000), now)).toBe('now');
		expect(formatRelativeTime(ago(5 * 60_000), now)).toBe('5m');
		expect(formatRelativeTime(ago(3 * 3_600_000), now)).toBe('3h');
		expect(formatRelativeTime(ago(2 * 86_400_000), now)).toBe('2d');
		expect(formatRelativeTime(ago(15 * 86_400_000), now)).toBe('2w');
	});

	it('falls back to a short date after a month', () => {
		expect(formatRelativeTime('2026-03-03T12:00:00Z', now)).toBe('Mar 3');
	});

	it('returns empty string for invalid input', () => {
		expect(formatRelativeTime('nope', now)).toBe('');
	});
});

describe('plural', () => {
	it('pluralizes by count', () => {
		expect(plural(1, 'file')).toBe('1 file');
		expect(plural(0, 'file')).toBe('0 files');
		expect(plural(3, 'commit')).toBe('3 commits');
	});
});
