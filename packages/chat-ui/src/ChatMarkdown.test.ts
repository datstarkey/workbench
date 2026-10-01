import { describe, expect, it } from 'vitest';
import { safeHref } from './ChatMarkdown.svelte';

describe('chat markdown links', () => {
	it('only lets web and mail links leave the app', () => {
		expect(safeHref('https://example.com/a')).toBe('https://example.com/a');
		expect(safeHref(' mailto:me@example.com ')).toBe('mailto:me@example.com');
		expect(safeHref('javascript:alert(1)')).toBe(null);
		expect(safeHref('file:///etc/passwd')).toBe(null);
		expect(safeHref('tauri://localhost')).toBe(null);
	});
});
