import { beforeEach, describe, expect, it } from 'vitest';
import { createMockTransport, type MockTransport } from '@workbench/transport';
import { ControlPlaneStore } from './control-plane.svelte.ts';

describe('ControlPlaneStore', () => {
	let transport: MockTransport;
	let store: ControlPlaneStore;

	beforeEach(() => {
		transport = createMockTransport();
		store = new ControlPlaneStore(transport);
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

	it('captures errors from the transport into store.error', async () => {
		transport.mockInvoke('list_projects', () => {
			throw new Error('network down');
		});
		await store.loadProjects();
		expect(store.error).toBe('network down');
		expect(store.projects).toEqual([]);
	});
});
