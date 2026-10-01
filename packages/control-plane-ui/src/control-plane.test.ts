import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createMockTransport, type MockTransport } from '@workbench/transport';
import { ControlPlaneStore } from './control-plane.svelte.ts';

function session(over: Record<string, unknown> = {}) {
	return {
		id: 'sess-1',
		name: 'test',
		cwd: '/p',
		pid: 123,
		status: 'starting',
		sessionUrl: null,
		startedAt: 0,
		...over
	};
}

describe('ControlPlaneStore', () => {
	let transport: MockTransport;
	let store: ControlPlaneStore;

	beforeEach(() => {
		transport = createMockTransport();
		store = new ControlPlaneStore(transport);
	});

	afterEach(() => {
		vi.useRealTimers();
	});

	it('loadProjects populates projects', async () => {
		transport.mockInvoke('list_projects', () => [{ name: 'a', path: '/a' }]);
		await store.loadProjects();
		expect(store.projects).toEqual([{ name: 'a', path: '/a' }]);
	});

	it('loadWorktrees stores per-project worktrees', async () => {
		transport.mockInvoke('list_worktrees', () => [{ path: '/a/wt', branch: 'x', isMain: false }]);
		await store.loadWorktrees('/a');
		expect(store.worktrees['/a']).toHaveLength(1);
	});

	describe('loadGithubUrl', () => {
		let calls: number;
		const remote = (result: unknown) =>
			transport.mockInvoke('github_get_remote', () => {
				calls++;
				return result;
			});

		beforeEach(() => (calls = 0));

		it('stores the repo web URL and loads each path once', async () => {
			remote({ owner: 'o', repo: 'r', htmlUrl: 'https://github.com/o/r' });
			await store.loadGithubUrl('/a');
			await store.loadGithubUrl('/a');
			expect(store.githubUrls['/a']).toBe('https://github.com/o/r');
			expect(calls).toBe(1);
		});

		it('caches null for a folder without a GitHub origin', async () => {
			remote(null);
			await store.loadGithubUrl('/a');
			await store.loadGithubUrl('/a');
			expect(store.githubUrls['/a']).toBeNull();
			expect(calls).toBe(1);
		});

		it('leaves a failed request uncached, without an error, so the next call retries', async () => {
			transport.mockInvoke('github_get_remote', () => {
				throw new Error('offline');
			});
			await store.loadGithubUrl('/a');
			expect('/a' in store.githubUrls).toBe(false);
			expect(store.error).toBeNull();

			remote({ owner: 'o', repo: 'r', htmlUrl: 'https://github.com/o/r' });
			await store.loadGithubUrl('/a');
			expect(store.githubUrls['/a']).toBe('https://github.com/o/r');
		});

		it('collapses concurrent calls for one path into one request', async () => {
			remote(null);
			await Promise.all([store.loadGithubUrl('/a'), store.loadGithubUrl('/a')]);
			expect(calls).toBe(1);
		});

		it('is re-fetched by refresh()', async () => {
			remote(null);
			transport.mockInvoke('list_projects', () => []);
			transport.mockInvoke('remote_sessions', () => []);
			await store.loadGithubUrl('/a');

			remote({ owner: 'o', repo: 'r', htmlUrl: 'https://github.com/o/r' });
			await store.refresh();
			expect(calls).toBe(2);
			expect(store.githubUrls).toEqual({ '/a': 'https://github.com/o/r' });
		});
	});

	it('createWorktree reloads worktrees on success', async () => {
		const calls: string[] = [];
		transport.mockInvoke('create_worktree', () => {
			calls.push('create');
			return '/a/feature';
		});
		transport.mockInvoke('list_worktrees', () => {
			calls.push('list');
			return [];
		});
		const result = await store.createWorktree('/a', 'feature');
		expect(result).toBe('/a/feature');
		expect(calls).toEqual(['create', 'list']);
	});

	it('spawn refreshes sessions and polls for the async URL update', async () => {
		vi.useFakeTimers();
		transport.mockInvoke('remote_spawn', () => session());
		let listCalls = 0;
		transport.mockInvoke('remote_sessions', () => {
			listCalls++;
			return [session({ status: listCalls > 2 ? 'running' : 'starting' })];
		});

		await store.spawn('/p', undefined, 'test');
		expect(listCalls).toBe(1); // immediate refresh after spawn

		// Polls every 2s up to 5 times.
		await vi.advanceTimersByTimeAsync(2000);
		await vi.advanceTimersByTimeAsync(2000);
		expect(listCalls).toBeGreaterThanOrEqual(3);
		expect(store.sessions[0].status).toBe('running');
	});

	it('killSession invokes remote_kill then refreshes', async () => {
		const order: string[] = [];
		transport.mockInvoke('remote_kill', () => {
			order.push('kill');
		});
		transport.mockInvoke('remote_sessions', () => {
			order.push('refresh');
			return [];
		});
		await store.killSession('sess-1');
		expect(order).toEqual(['kill', 'refresh']);
		expect(store.sessions).toEqual([]);
	});

	it('captures errors from the transport into store.error', async () => {
		transport.mockInvoke('list_projects', () => {
			throw new Error('network down');
		});
		await store.loadProjects();
		expect(store.error).toBe('network down');
		expect(store.projects).toEqual([]);
	});

	it('dispose() cancels the spawn status poll so it stops hitting the server', async () => {
		vi.useFakeTimers();
		transport.mockInvoke('remote_spawn', () => session());
		let listCalls = 0;
		transport.mockInvoke('remote_sessions', () => {
			listCalls++;
			return [session()];
		});

		await store.spawn('/p', undefined, 'test');
		expect(listCalls).toBe(1); // immediate refresh after spawn

		store.dispose(); // cancel the still-running poll
		await vi.advanceTimersByTimeAsync(10000);
		expect(listCalls).toBe(1); // no further polling after dispose
	});
});
