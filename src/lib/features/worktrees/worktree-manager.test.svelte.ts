import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invokeSpy, mockInvoke, clearInvokeMocks } from '../../../test/tauri-mocks';
import { WorktreeManagerStore } from './worktree-manager.svelte';
import type { GitStore } from '$stores/git.svelte';
import type { ProjectStore } from '$stores/projects.svelte';
import type { WorkspaceStore } from '$stores/workspaces.svelte';
import type { BranchInfo, ProjectConfig, ProjectWorkspace } from '$types/workbench';

function makeProject(overrides: Partial<ProjectConfig> = {}): ProjectConfig {
	return { name: 'Test', path: '/projects/test', ...overrides };
}

function createMocks() {
	const projectStore = {
		getByPath: vi.fn()
	} as unknown as ProjectStore;

	const workspaceStore = {
		openWorktree: vi.fn(),
		getByWorktreePath: vi.fn(),
		close: vi.fn()
	} as unknown as WorkspaceStore;

	const gitStore = {
		refreshGitState: vi.fn()
	} as unknown as GitStore;

	return { projectStore, workspaceStore, gitStore };
}

describe('WorktreeManagerStore', () => {
	let manager: WorktreeManagerStore;
	let mocks: ReturnType<typeof createMocks>;

	beforeEach(() => {
		mocks = createMocks();
		manager = new WorktreeManagerStore(mocks.projectStore, mocks.workspaceStore, mocks.gitStore);
	});

	afterEach(() => {
		clearInvokeMocks();
	});

	describe('add', () => {
		it('opens dialog and fetches branches via invoke', async () => {
			const branches: BranchInfo[] = [
				{ name: 'main', sha: 'abc123', isCurrent: true, isRemote: false },
				{ name: 'feature', sha: 'def456', isCurrent: false, isRemote: false }
			];
			mockInvoke('list_branches', () => branches);

			await manager.add('/projects/repo');

			expect(manager.dialogOpen).toBe(true);
			expect(manager.dialogProjectPath).toBe('/projects/repo');
			expect(manager.dialogError).toBe('');
			expect(invokeSpy).toHaveBeenCalledWith('list_branches', { path: '/projects/repo' });
			expect(manager.dialogBranches).toEqual(branches);
		});

		it('handles branch listing failure', async () => {
			mockInvoke('list_branches', () => {
				throw new Error('git error');
			});

			await manager.add('/projects/repo');

			expect(manager.dialogOpen).toBe(true);
			expect(manager.dialogError).toBe('Failed to list branches: Error: git error');
			expect(manager.dialogBranches).toEqual([]);
		});

		it('resets state before opening', async () => {
			manager.dialogError = 'old error';
			manager.dialogBranches = [{ name: 'stale', sha: '000', isCurrent: false, isRemote: false }];
			mockInvoke('list_branches', () => []);

			await manager.add('/projects/new');

			expect(manager.dialogProjectPath).toBe('/projects/new');
			expect(manager.dialogError).toBe('');
		});
	});

	describe('create', () => {
		it('invokes create_worktree, refreshes git, and opens workspace', async () => {
			const project = makeProject({ name: 'Repo', path: '/projects/repo' });
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(project);
			vi.mocked(mocks.gitStore.refreshGitState).mockResolvedValue();
			mockInvoke('create_worktree', () => '/projects/repo-wt');

			manager.dialogProjectPath = '/projects/repo';
			manager.dialogOpen = true;

			await manager.create('feature', true, '/projects/repo-wt', {
				aiConfig: true,
				envFiles: false
			});

			expect(invokeSpy).toHaveBeenCalledWith('create_worktree', {
				request: {
					repoPath: '/projects/repo',
					branch: 'feature',
					newBranch: true,
					path: '/projects/repo-wt',
					copyOptions: { aiConfig: true, envFiles: false }
				}
			});
			expect(manager.dialogOpen).toBe(false);
			expect(mocks.gitStore.refreshGitState).toHaveBeenCalledWith('/projects/repo');
			expect(mocks.workspaceStore.openWorktree).toHaveBeenCalledWith(
				project,
				'/projects/repo-wt',
				'feature'
			);
		});

		it('handles failure and sets dialogError', async () => {
			mockInvoke('create_worktree', () => {
				throw new Error('worktree already exists');
			});

			manager.dialogProjectPath = '/projects/repo';
			manager.dialogOpen = true;

			await manager.create('feature', false, '/projects/repo-wt', {
				aiConfig: false,
				envFiles: false
			});

			expect(manager.dialogError).toBe('Error: worktree already exists');
			expect(manager.dialogOpen).toBe(true);
		});

		it('does not open workspace if project not found', async () => {
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(undefined);
			vi.mocked(mocks.gitStore.refreshGitState).mockResolvedValue();
			mockInvoke('create_worktree', () => '/projects/repo-wt');

			manager.dialogProjectPath = '/projects/repo';

			await manager.create('feature', true, '/projects/repo-wt', {
				aiConfig: false,
				envFiles: false
			});

			expect(mocks.workspaceStore.openWorktree).not.toHaveBeenCalled();
		});
	});

	describe('open', () => {
		it('delegates to workspaceStore.openWorktree', () => {
			const project = makeProject({ name: 'Repo', path: '/projects/repo' });
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(project);

			manager.open('/projects/repo', '/projects/repo-wt', 'feature');

			expect(mocks.workspaceStore.openWorktree).toHaveBeenCalledWith(
				project,
				'/projects/repo-wt',
				'feature'
			);
		});

		it('does nothing if project not found', () => {
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(undefined);

			manager.open('/nonexistent', '/wt', 'branch');

			expect(mocks.workspaceStore.openWorktree).not.toHaveBeenCalled();
		});
	});

	describe('remove / confirmRemove', () => {
		it('remove() opens the removal ConfirmAction', () => {
			manager.remove('/projects/repo', '/projects/repo-wt');

			expect(manager.removal.open).toBe(true);
			expect(manager.removal.pendingValue).toEqual({
				projectPath: '/projects/repo',
				worktreePath: '/projects/repo-wt'
			});
		});

		it('confirmRemove() invokes remove_worktree, closes workspace, and refreshes git', async () => {
			const ws = { id: 'ws-1' } as ProjectWorkspace;
			vi.mocked(mocks.workspaceStore.getByWorktreePath).mockReturnValue(ws);
			vi.mocked(mocks.gitStore.refreshGitState).mockResolvedValue();

			manager.remove('/projects/repo', '/projects/repo-wt');
			await manager.confirmRemove();

			expect(invokeSpy).toHaveBeenCalledWith('remove_worktree', {
				repoPath: '/projects/repo',
				worktreePath: '/projects/repo-wt'
			});
			expect(mocks.workspaceStore.getByWorktreePath).toHaveBeenCalledWith('/projects/repo-wt');
			expect(mocks.workspaceStore.close).toHaveBeenCalledWith('ws-1');
			expect(mocks.gitStore.refreshGitState).toHaveBeenCalledWith('/projects/repo');
			expect(manager.removal.open).toBe(false);
		});

		it('confirmRemove() skips workspace close if no matching workspace', async () => {
			vi.mocked(mocks.workspaceStore.getByWorktreePath).mockReturnValue(undefined);
			vi.mocked(mocks.gitStore.refreshGitState).mockResolvedValue();

			manager.remove('/projects/repo', '/projects/repo-wt');
			await manager.confirmRemove();

			expect(invokeSpy).toHaveBeenCalledWith('remove_worktree', {
				repoPath: '/projects/repo',
				worktreePath: '/projects/repo-wt'
			});
			expect(mocks.workspaceStore.close).not.toHaveBeenCalled();
		});

		it('confirmRemove() does nothing if no pending removal', async () => {
			await manager.confirmRemove();

			expect(invokeSpy).not.toHaveBeenCalled();
		});

		it('confirmRemove() sets error on failure', async () => {
			mockInvoke('remove_worktree', () => {
				throw new Error('removal failed');
			});

			manager.remove('/projects/repo', '/projects/repo-wt');
			await manager.confirmRemove();

			expect(manager.removal.error).toBe('Error: removal failed');
			expect(manager.removal.open).toBe(true);
		});
	});
});
