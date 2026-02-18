import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
	invokeSpy,
	mockInvoke,
	clearInvokeMocks,
	emitMockEvent,
	clearListeners,
	listenSpy
} from '../../test/tauri-mocks';
import { GitStore } from './git.svelte';
import type { WorktreeInfo } from '$types/workbench';

describe('GitStore', () => {
	let store: GitStore;

	beforeEach(() => {
		store = new GitStore();
	});

	afterEach(() => {
		clearInvokeMocks();
		clearListeners();
	});

	describe('constructor', () => {
		it('registers a project:refresh-requested listener', () => {
			expect(listenSpy).toHaveBeenCalledWith('project:refresh-requested', expect.any(Function));
		});

		it('does not register legacy git:changed listener', () => {
			expect(listenSpy).not.toHaveBeenCalledWith('git:changed', expect.any(Function));
		});
	});

	describe('project:refresh-requested refresh', () => {
		it('refreshes project immediately', () => {
			const refreshSpy = vi.spyOn(store, 'refreshGitState').mockResolvedValue();
			emitMockEvent('project:refresh-requested', {
				projectPath: '/projects/repo',
				source: 'claude-hook',
				trigger: 'post-tool-use-bash'
			});

			expect(refreshSpy).toHaveBeenCalledWith('/projects/repo');
		});

		it('refreshes for each refresh event', () => {
			const refreshSpy = vi.spyOn(store, 'refreshGitState').mockResolvedValue();
			emitMockEvent('project:refresh-requested', {
				projectPath: '/projects/repo',
				source: 'claude-hook',
				trigger: 'post-tool-use-bash'
			});
			emitMockEvent('project:refresh-requested', {
				projectPath: '/projects/repo',
				source: 'git-watcher',
				trigger: 'git-dir-change'
			});

			expect(refreshSpy).toHaveBeenCalledTimes(2);
		});
	});

	describe('fetchGitInfo', () => {
		it('sets branchByProject on success', async () => {
			mockInvoke('git_info', () => ({
				branch: 'main',
				repoRoot: '/projects/foo',
				isWorktree: false
			}));

			await store.fetchGitInfo('/projects/foo');

			expect(invokeSpy).toHaveBeenCalledWith('git_info', { path: '/projects/foo' });
			expect(store.branchByProject['/projects/foo']).toBe('main');
		});

		it('preserves existing branches when adding a new one', async () => {
			mockInvoke('git_info', () => ({
				branch: 'dev',
				repoRoot: '/projects/bar',
				isWorktree: false
			}));

			store.branchByProject = { '/projects/existing': 'main' };
			await store.fetchGitInfo('/projects/bar');

			expect(store.branchByProject['/projects/existing']).toBe('main');
			expect(store.branchByProject['/projects/bar']).toBe('dev');
		});

		it('warns on failure and does not modify state', async () => {
			const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
			mockInvoke('git_info', () => {
				throw new Error('not a git repo');
			});

			await store.fetchGitInfo('/projects/bad');

			expect(warnSpy).toHaveBeenCalledWith(
				'[GitStore] Not a git repo or git info failed:',
				expect.any(Error)
			);
			expect(store.branchByProject['/projects/bad']).toBeUndefined();
			warnSpy.mockRestore();
		});
	});

	describe('fetchWorktrees', () => {
		it('sets worktreesByProject on success', async () => {
			const worktrees: WorktreeInfo[] = [
				{ path: '/projects/foo', head: 'abc123', branch: 'main', isMain: true },
				{ path: '/projects/foo-wt', head: 'def456', branch: 'feature', isMain: false }
			];
			mockInvoke('list_worktrees', () => worktrees);

			await store.fetchWorktrees('/projects/foo');

			expect(invokeSpy).toHaveBeenCalledWith('list_worktrees', { path: '/projects/foo' });
			expect(store.worktreesByProject['/projects/foo']).toEqual(worktrees);
		});

		it('preserves existing worktrees when adding new ones', async () => {
			const existing: WorktreeInfo[] = [
				{ path: '/projects/a', head: 'aaa', branch: 'main', isMain: true }
			];
			store.worktreesByProject = { '/projects/a': existing };

			const newWts: WorktreeInfo[] = [
				{ path: '/projects/b', head: 'bbb', branch: 'dev', isMain: true }
			];
			mockInvoke('list_worktrees', () => newWts);

			await store.fetchWorktrees('/projects/b');

			expect(store.worktreesByProject['/projects/a']).toEqual(existing);
			expect(store.worktreesByProject['/projects/b']).toEqual(newWts);
		});

		it('warns on failure and does not modify state', async () => {
			const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
			mockInvoke('list_worktrees', () => {
				throw new Error('failed');
			});

			await store.fetchWorktrees('/projects/bad');

			expect(warnSpy).toHaveBeenCalledWith(
				'[GitStore] Failed to list worktrees:',
				expect.any(Error)
			);
			expect(store.worktreesByProject['/projects/bad']).toBeUndefined();
			warnSpy.mockRestore();
		});
	});

	describe('refreshGitState', () => {
		it('calls both fetchGitInfo and fetchWorktrees', async () => {
			const gitInfoSpy = vi.spyOn(store, 'fetchGitInfo').mockResolvedValue();
			const worktreesSpy = vi.spyOn(store, 'fetchWorktrees').mockResolvedValue();

			await store.refreshGitState('/projects/foo');

			expect(gitInfoSpy).toHaveBeenCalledWith('/projects/foo');
			expect(worktreesSpy).toHaveBeenCalledWith('/projects/foo');
		});
	});

	describe('refreshAll', () => {
		it('calls refreshGitState for each path', async () => {
			const spy = vi.spyOn(store, 'refreshGitState').mockResolvedValue();

			await store.refreshAll(['/projects/a', '/projects/b', '/projects/c']);

			expect(spy).toHaveBeenCalledTimes(3);
			expect(spy).toHaveBeenCalledWith('/projects/a');
			expect(spy).toHaveBeenCalledWith('/projects/b');
			expect(spy).toHaveBeenCalledWith('/projects/c');
		});

		it('handles empty array', async () => {
			const spy = vi.spyOn(store, 'refreshGitState').mockResolvedValue();
			await store.refreshAll([]);
			expect(spy).not.toHaveBeenCalled();
		});
	});
});
