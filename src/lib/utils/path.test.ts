import { describe, it, expect } from 'vitest';
import { baseName, effectivePath } from './path';

describe('baseName', () => {
	it('extracts last segment from Unix path', () => {
		expect(baseName('/Users/jake/projects/my-app')).toBe('my-app');
	});

	it('extracts last segment from Windows path', () => {
		expect(baseName('C:\\Users\\jake\\projects\\my-app')).toBe('my-app');
	});

	it('handles trailing slash', () => {
		expect(baseName('/Users/jake/projects/')).toBe('projects');
	});

	it('handles trailing backslash', () => {
		expect(baseName('C:\\Users\\jake\\projects\\')).toBe('projects');
	});

	it('returns single segment as-is', () => {
		expect(baseName('my-app')).toBe('my-app');
	});

	it('returns empty string for empty input', () => {
		expect(baseName('')).toBe('');
	});

	it('handles deeply nested path', () => {
		expect(baseName('/a/b/c/d/e/f')).toBe('f');
	});

	it('handles mixed separators', () => {
		expect(baseName('C:\\Users/jake\\projects/my-app')).toBe('my-app');
	});
});

describe('effectivePath', () => {
	it('returns worktreePath when present', () => {
		const ws = { projectPath: '/main', worktreePath: '/worktree' };
		expect(effectivePath(ws)).toBe('/worktree');
	});

	it('returns projectPath when worktreePath is undefined', () => {
		const ws = { projectPath: '/main' };
		expect(effectivePath(ws)).toBe('/main');
	});

	it('returns projectPath when worktreePath is explicitly undefined', () => {
		const ws = { projectPath: '/main', worktreePath: undefined };
		expect(effectivePath(ws)).toBe('/main');
	});
});
