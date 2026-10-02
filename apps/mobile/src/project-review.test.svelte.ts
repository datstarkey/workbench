import { expect, it } from 'vitest';
import { createMockTransport } from '@workbench/transport';
import { ProjectReview } from './project-review.svelte';

it('filters Claude history by account and sorts newest first; Codex is independent', async () => {
	const transport = createMockTransport();
	transport.mockInvoke('discover_claude_sessions', () => [
		{ sessionId: 'default', label: 'Default', timestamp: '2026-01-01' },
		{ sessionId: 'old', label: 'Old', accountId: 'work', timestamp: '2026-01-01' },
		{ sessionId: 'new', label: 'New', accountId: 'work', timestamp: '2026-02-01' }
	]);
	transport.mockInvoke('discover_codex_sessions', () => [
		{ sessionId: 'codex', timestamp: '2026-03-01' }
	]);
	const review = new ProjectReview(transport, { projectPath: '/repo', name: 'repo' });
	await review.history('claude', 'work');
	expect(review.sessions.map((s) => s.sessionId)).toEqual(['new', 'old']);
	await review.history('codex', 'work');
	expect(review.sessions[0].sessionId).toBe('codex');
});

it('drops late history from an earlier selection and reports errors', async () => {
	const transport = createMockTransport();
	let resolve!: (list: unknown[]) => void;
	transport.mockInvoke(
		'discover_claude_sessions',
		() =>
			new Promise((r) => {
				resolve = r;
			})
	);
	transport.mockInvoke('discover_codex_sessions', () => [
		{ sessionId: 'codex', timestamp: '2026-03-01' }
	]);
	const review = new ProjectReview(transport, { projectPath: '/repo', name: 'repo' });
	const old = review.history('claude');
	await review.history('codex');
	resolve([{ sessionId: 'old' }]);
	await old;
	expect(review.sessions[0].sessionId).toBe('codex');
	transport.mockInvoke('git_status', () => {
		throw new Error('offline');
	});
	await review.changes();
	expect(review.error).toBe('offline');
	expect(review.loading).toBe(false);
});

it('requests the correct checkout and staged version', async () => {
	const transport = createMockTransport();
	transport.mockInvoke('git_file_diff', (args) => {
		expect(args).toEqual({ path: '/wt', projectPath: '/repo', file: 'a.ts', staged: true });
		return '+new';
	});
	const review = new ProjectReview(transport, {
		projectPath: '/repo',
		worktreePath: '/wt',
		name: 'wt'
	});
	await review.preview({ path: 'a.ts', status: 'M', staged: true, unstaged: false }, true);
	expect(review.diff).toBe('+new');
});
