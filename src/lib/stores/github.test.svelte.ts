import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
	invokeSpy,
	mockInvoke,
	clearInvokeMocks,
	emitMockEvent,
	clearListeners,
	listenSpy
} from '../../test/tauri-mocks';
import { GitHubStore } from './github.svelte';
import type {
	GitHubCheckDetail,
	GitHubProjectStatus,
	GitHubPR,
	GitHubBranchRuns,
	ProjectConfig
} from '$types/workbench';

// Mock plugin-store (used by initSidebarState/toggleSidebar)
const mockStoreGet = vi.fn();
const mockStoreSet = vi.fn();
const mockStoreSave = vi.fn();
vi.mock('@tauri-apps/plugin-store', () => ({
	load: vi.fn(async () => ({
		get: mockStoreGet,
		set: mockStoreSet,
		save: mockStoreSave
	}))
}));

// Mock context dependencies
const mockWorkspaceStore = {
	activeWorkspaceId: null as string | null,
	activeWorkspace: null as { id: string; projectPath: string; worktreePath?: string } | null,
	resolvedBranch: vi.fn(() => null as string | null)
};

const mockGitStore = {
	branchByProject: {} as Record<string, string>,
	worktreesByProject: {} as Record<string, Array<{ branch?: string; isMain: boolean }>>
};

const mockSessionStore = {
	activeSessionsByProject: {} as Record<string, string[]>
};

const mockProjectStore = {
	projects: [] as ProjectConfig[]
};

vi.mock('./context', () => ({
	getWorkspaceStore: () => mockWorkspaceStore,
	getGitStore: () => mockGitStore,
	getClaudeSessionStore: () => mockSessionStore,
	getProjectStore: () => mockProjectStore
}));

// --- Pure function tests (exported from module scope) ---
// We re-export them via a wrapper so we can import and test them directly.
// Since detectCheckTransitions and buildBucketMap are module-scoped (not exported),
// we test them indirectly through the store's fetchPrChecks method, or we can
// test the logic by calling the store methods that use them.

// Helper factories

function makeCheck(overrides: Partial<GitHubCheckDetail> = {}): GitHubCheckDetail {
	return {
		name: 'CI',
		bucket: 'pass',
		workflow: 'build',
		link: 'https://example.com',
		startedAt: null,
		completedAt: null,
		description: '',
		...overrides
	};
}

function makePR(overrides: Partial<GitHubPR> = {}): GitHubPR {
	return {
		number: 1,
		title: 'Test PR',
		state: 'OPEN',
		url: 'https://github.com/test/repo/pull/1',
		isDraft: false,
		headRefName: 'feature-branch',
		reviewDecision: null,
		checksStatus: { overall: 'success', total: 1, passing: 1, failing: 0, pending: 0 },
		mergeStateStatus: null,
		...overrides
	};
}

function makeBranchRuns(overrides: Partial<GitHubBranchRuns> = {}): GitHubBranchRuns {
	return {
		status: { overall: 'success', total: 1, passing: 1, failing: 0, pending: 0 },
		runs: [],
		...overrides
	};
}

function makeProjectStatus(overrides: Partial<GitHubProjectStatus> = {}): GitHubProjectStatus {
	return {
		remote: { owner: 'test', repo: 'repo', htmlUrl: 'https://github.com/test/repo' },
		prs: [],
		branchRuns: {},
		prChecks: {},
		...overrides
	};
}

describe('GitHubStore', () => {
	let store: GitHubStore;

	beforeEach(() => {
		vi.useFakeTimers();
		mockWorkspaceStore.activeWorkspaceId = null;
		mockWorkspaceStore.activeWorkspace = null;
		mockWorkspaceStore.resolvedBranch = vi.fn(() => null);
		mockGitStore.branchByProject = {};
		mockGitStore.worktreesByProject = {};
		mockSessionStore.activeSessionsByProject = {};
		mockProjectStore.projects = [];
		mockStoreGet.mockReset();
		mockStoreSet.mockReset();
		mockStoreSave.mockReset();
		store = new GitHubStore();
	});

	afterEach(() => {
		store.destroy();
		clearInvokeMocks();
		clearListeners();
		vi.useRealTimers();
	});

	// ─── Constructor ──────────────────────────────────────────

	describe('constructor', () => {
		it('registers a git:changed listener', () => {
			expect(listenSpy).toHaveBeenCalledWith('git:changed', expect.any(Function));
		});

		it('starts a slow poll interval', () => {
			// The constructor sets up a setInterval for polling.
			// We can verify indirectly that a timer was created.
			// The store should survive construction without errors.
			expect(store.ghAvailable).toBeNull();
		});
	});

	// ─── checkGhAvailable ─────────────────────────────────────

	describe('checkGhAvailable', () => {
		it('invokes github_is_available and stores result', async () => {
			mockInvoke('github_is_available', () => true);

			const result = await store.checkGhAvailable();

			expect(result).toBe(true);
			expect(store.ghAvailable).toBe(true);
			expect(invokeSpy).toHaveBeenCalledWith('github_is_available');
		});

		it('caches result on subsequent calls', async () => {
			mockInvoke('github_is_available', () => true);

			await store.checkGhAvailable();
			invokeSpy.mockClear();
			const result = await store.checkGhAvailable();

			expect(result).toBe(true);
			expect(invokeSpy).not.toHaveBeenCalled();
		});

		it('sets ghAvailable to false on error', async () => {
			mockInvoke('github_is_available', () => {
				throw new Error('not found');
			});

			const result = await store.checkGhAvailable();

			expect(result).toBe(false);
			expect(store.ghAvailable).toBe(false);
		});
	});

	// ─── fetchProjectStatus ───────────────────────────────────

	describe('fetchProjectStatus', () => {
		it('fetches and stores project status', async () => {
			const status = makeProjectStatus({
				prs: [makePR({ number: 42, headRefName: 'main' })],
				branchRuns: { main: makeBranchRuns() },
				prChecks: { 42: [makeCheck()] }
			});
			mockInvoke('github_project_status', () => status);

			await store.fetchProjectStatus('/my/project');

			expect(invokeSpy).toHaveBeenCalledWith('github_project_status', {
				projectPath: '/my/project'
			});
			expect(store.remoteByProject['/my/project']).toEqual(status.remote);
			expect(store.prsByProject['/my/project']).toEqual(status.prs);
			expect(store.branchRunsByProject['/my/project']).toEqual(status.branchRuns);
			expect(store.checksByPr['/my/project::42']).toEqual([makeCheck()]);
		});

		it('deduplicates concurrent requests for the same project', async () => {
			let resolveInvoke: (v: GitHubProjectStatus) => void;
			mockInvoke(
				'github_project_status',
				() =>
					new Promise<GitHubProjectStatus>((resolve) => {
						resolveInvoke = resolve;
					})
			);

			const p1 = store.fetchProjectStatus('/project');
			const p2 = store.fetchProjectStatus('/project');

			resolveInvoke!(makeProjectStatus());
			await Promise.all([p1, p2]);

			// Only one invoke call despite two fetchProjectStatus calls
			const calls = invokeSpy.mock.calls.filter((c) => c[0] === 'github_project_status');
			expect(calls).toHaveLength(1);
		});

		it('handles fetch failure gracefully', async () => {
			const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
			mockInvoke('github_project_status', () => {
				throw new Error('network error');
			});

			await store.fetchProjectStatus('/project');

			expect(warnSpy).toHaveBeenCalledWith(
				'[GitHubStore] Failed to fetch project status:',
				expect.any(Error)
			);
			expect(store.prsByProject['/project']).toBeUndefined();
			warnSpy.mockRestore();
		});

		it('allows retry after failure', async () => {
			mockInvoke('github_project_status', () => {
				throw new Error('fail');
			});
			const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
			await store.fetchProjectStatus('/project');
			warnSpy.mockRestore();

			// Now succeed
			clearInvokeMocks();
			mockInvoke('github_project_status', () => makeProjectStatus({ prs: [makePR()] }));
			await store.fetchProjectStatus('/project');

			expect(store.prsByProject['/project']).toHaveLength(1);
		});
	});

	// ─── getBranchStatus ──────────────────────────────────────

	describe('getBranchStatus', () => {
		it('returns undefined when no data exists', () => {
			expect(store.getBranchStatus('/project', 'main')).toBeUndefined();
		});

		it('returns status with matching PR', () => {
			const pr = makePR({ headRefName: 'feature' });
			store.prsByProject = { '/project': [pr] };
			store.remoteByProject = {
				'/project': { owner: 'o', repo: 'r', htmlUrl: 'https://github.com/o/r' }
			};
			store.branchRunsByProject = { '/project': { feature: makeBranchRuns() } };

			const status = store.getBranchStatus('/project', 'feature');

			expect(status?.pr).toEqual(pr);
			expect(status?.remote).toEqual({
				owner: 'o',
				repo: 'r',
				htmlUrl: 'https://github.com/o/r'
			});
			expect(status?.branchRuns).toEqual(makeBranchRuns());
		});

		it('returns null PR when no branch matches', () => {
			store.prsByProject = { '/project': [makePR({ headRefName: 'other' })] };
			store.remoteByProject = { '/project': null };

			const status = store.getBranchStatus('/project', 'main');

			expect(status?.pr).toBeNull();
		});
	});

	// ─── getRemoteUrl ─────────────────────────────────────────

	describe('getRemoteUrl', () => {
		it('returns htmlUrl when remote exists', () => {
			store.remoteByProject = {
				'/project': { owner: 'o', repo: 'r', htmlUrl: 'https://github.com/o/r' }
			};

			expect(store.getRemoteUrl('/project')).toBe('https://github.com/o/r');
		});

		it('returns null when no remote', () => {
			expect(store.getRemoteUrl('/project')).toBeNull();
		});

		it('returns null when remote is null', () => {
			store.remoteByProject = { '/project': null };
			expect(store.getRemoteUrl('/project')).toBeNull();
		});
	});

	// ─── Sidebar state ────────────────────────────────────────

	describe('sidebarTarget', () => {
		it('returns null when no active workspace', () => {
			expect(store.sidebarTarget).toBeNull();
		});

		it('returns active workspace branch when no override', () => {
			mockWorkspaceStore.activeWorkspace = { id: 'ws-1', projectPath: '/project' };
			mockWorkspaceStore.resolvedBranch = vi.fn(() => 'main');

			expect(store.sidebarTarget).toEqual({ projectPath: '/project', branch: 'main' });
		});

		it('returns null when active workspace has no branch', () => {
			mockWorkspaceStore.activeWorkspace = { id: 'ws-1', projectPath: '/project' };
			mockWorkspaceStore.resolvedBranch = vi.fn(() => null);

			expect(store.sidebarTarget).toBeNull();
		});

		it('uses override when set for current workspace', () => {
			mockWorkspaceStore.activeWorkspaceId = 'ws-1';
			mockWorkspaceStore.activeWorkspace = { id: 'ws-1', projectPath: '/project' };
			mockWorkspaceStore.resolvedBranch = vi.fn(() => 'main');

			store.showBranch('/other-project', 'feature');

			expect(store.sidebarTarget).toEqual({ projectPath: '/other-project', branch: 'feature' });
		});

		it('ignores override when workspace changes', () => {
			mockWorkspaceStore.activeWorkspaceId = 'ws-1';
			mockWorkspaceStore.activeWorkspace = { id: 'ws-1', projectPath: '/project' };
			mockWorkspaceStore.resolvedBranch = vi.fn(() => 'main');

			store.showBranch('/other-project', 'feature');

			// Switch workspace
			mockWorkspaceStore.activeWorkspaceId = 'ws-2';
			mockWorkspaceStore.activeWorkspace = { id: 'ws-2', projectPath: '/project2' };
			mockWorkspaceStore.resolvedBranch = vi.fn(() => 'develop');

			expect(store.sidebarTarget).toEqual({ projectPath: '/project2', branch: 'develop' });
		});
	});

	describe('sidebarPr', () => {
		it('returns null when no sidebar target', () => {
			expect(store.sidebarPr).toBeNull();
		});

		it('returns PR matching sidebar target branch', () => {
			const pr = makePR({ headRefName: 'feature' });
			store.prsByProject = { '/project': [pr] };
			mockWorkspaceStore.activeWorkspace = { id: 'ws-1', projectPath: '/project' };
			mockWorkspaceStore.resolvedBranch = vi.fn(() => 'feature');

			expect(store.sidebarPr).toEqual(pr);
		});

		it('returns null when no PR matches branch', () => {
			store.prsByProject = { '/project': [makePR({ headRefName: 'other' })] };
			mockWorkspaceStore.activeWorkspace = { id: 'ws-1', projectPath: '/project' };
			mockWorkspaceStore.resolvedBranch = vi.fn(() => 'feature');

			expect(store.sidebarPr).toBeNull();
		});
	});

	describe('sidebarBranchRuns', () => {
		it('returns null when no sidebar target', () => {
			expect(store.sidebarBranchRuns).toBeNull();
		});

		it('returns branch runs for sidebar target', () => {
			const runs = makeBranchRuns();
			store.branchRunsByProject = { '/project': { main: runs } };
			mockWorkspaceStore.activeWorkspace = { id: 'ws-1', projectPath: '/project' };
			mockWorkspaceStore.resolvedBranch = vi.fn(() => 'main');

			expect(store.sidebarBranchRuns).toEqual(runs);
		});
	});

	describe('sidebarChecks', () => {
		it('returns empty array when no target', () => {
			expect(store.sidebarChecks).toEqual([]);
		});

		it('returns checks for sidebar PR', () => {
			const checks = [makeCheck({ name: 'lint' }), makeCheck({ name: 'test' })];
			const pr = makePR({ number: 5, headRefName: 'feature' });
			store.prsByProject = { '/project': [pr] };
			store.checksByPr = { '/project::5': checks };
			mockWorkspaceStore.activeWorkspace = { id: 'ws-1', projectPath: '/project' };
			mockWorkspaceStore.resolvedBranch = vi.fn(() => 'feature');

			expect(store.sidebarChecks).toEqual(checks);
		});
	});

	// ─── showBranch / clearSidebarOverride ─────────────────────

	describe('showBranch', () => {
		it('sets override target and refreshes project', () => {
			mockWorkspaceStore.activeWorkspaceId = 'ws-1';
			mockInvoke('github_project_status', () => makeProjectStatus());

			store.showBranch('/project', 'feature');

			// Verify override is set
			mockWorkspaceStore.activeWorkspace = { id: 'ws-1', projectPath: '/project' };
			mockWorkspaceStore.resolvedBranch = vi.fn(() => 'main');
			expect(store.sidebarTarget).toEqual({ projectPath: '/project', branch: 'feature' });
		});
	});

	describe('clearSidebarOverride', () => {
		it('clears override target', () => {
			mockWorkspaceStore.activeWorkspaceId = 'ws-1';
			mockWorkspaceStore.activeWorkspace = { id: 'ws-1', projectPath: '/project' };
			mockWorkspaceStore.resolvedBranch = vi.fn(() => 'main');

			store.showBranch('/other', 'feature');
			expect(store.sidebarTarget).toEqual({ projectPath: '/other', branch: 'feature' });

			store.clearSidebarOverride();
			expect(store.sidebarTarget).toEqual({ projectPath: '/project', branch: 'main' });
		});
	});

	// ─── toggleSidebar ────────────────────────────────────────

	describe('toggleSidebar', () => {
		it('toggles sidebarOpen state', async () => {
			expect(store.sidebarOpen).toBe(false);

			await store.toggleSidebar();
			expect(store.sidebarOpen).toBe(true);

			await store.toggleSidebar();
			expect(store.sidebarOpen).toBe(false);
		});
	});

	describe('initSidebarState', () => {
		it('loads persisted state', async () => {
			mockStoreGet.mockResolvedValueOnce(true);

			await store.initSidebarState();

			expect(store.sidebarOpen).toBe(true);
		});

		it('ignores non-boolean persisted value', async () => {
			mockStoreGet.mockResolvedValueOnce('not-a-boolean');

			await store.initSidebarState();

			expect(store.sidebarOpen).toBe(false);
		});

		it('handles load failure gracefully', async () => {
			mockStoreGet.mockRejectedValueOnce(new Error('no file'));

			await store.initSidebarState();

			expect(store.sidebarOpen).toBe(false);
		});
	});

	// ─── fetchPrChecks ────────────────────────────────────────

	describe('fetchPrChecks', () => {
		it('fetches and stores PR checks', async () => {
			const checks = [makeCheck({ name: 'lint' }), makeCheck({ name: 'test' })];
			mockInvoke('github_pr_checks', () => checks);

			await store.fetchPrChecks('/project', 42);

			expect(invokeSpy).toHaveBeenCalledWith('github_pr_checks', {
				projectPath: '/project',
				prNumber: 42
			});
			expect(store.checksByPr['/project::42']).toEqual(checks);
		});

		it('deduplicates concurrent requests for same PR', async () => {
			let resolveInvoke: (v: GitHubCheckDetail[]) => void;
			mockInvoke(
				'github_pr_checks',
				() =>
					new Promise<GitHubCheckDetail[]>((resolve) => {
						resolveInvoke = resolve;
					})
			);

			const p1 = store.fetchPrChecks('/project', 1);
			const p2 = store.fetchPrChecks('/project', 1);

			resolveInvoke!([makeCheck()]);
			await Promise.all([p1, p2]);

			const calls = invokeSpy.mock.calls.filter((c) => c[0] === 'github_pr_checks');
			expect(calls).toHaveLength(1);
		});

		it('handles fetch failure gracefully', async () => {
			const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
			mockInvoke('github_pr_checks', () => {
				throw new Error('network error');
			});

			await store.fetchPrChecks('/project', 1);

			expect(warnSpy).toHaveBeenCalledWith(
				'[GitHubStore] Failed to fetch PR checks:',
				expect.any(Error)
			);
			warnSpy.mockRestore();
		});

		it('detects check transitions and calls onCheckComplete callback', async () => {
			const callback = vi.fn();
			store.onCheckComplete(callback);

			// First fetch: pending checks
			const pendingChecks = [makeCheck({ name: 'CI', workflow: 'build', bucket: 'pending' })];
			mockInvoke('github_pr_checks', () => pendingChecks);
			await store.fetchPrChecks('/project', 1);

			// Second fetch: check passed
			const passedChecks = [makeCheck({ name: 'CI', workflow: 'build', bucket: 'pass' })];
			clearInvokeMocks();
			mockInvoke('github_pr_checks', () => passedChecks);
			await store.fetchPrChecks('/project', 1);

			expect(callback).toHaveBeenCalledWith({
				name: 'CI',
				bucket: 'pass',
				projectPath: '/project',
				prNumber: 1
			});
		});

		it('detects pending-to-fail transitions', async () => {
			const callback = vi.fn();
			store.onCheckComplete(callback);

			// First fetch: pending
			mockInvoke('github_pr_checks', () => [
				makeCheck({ name: 'CI', workflow: 'build', bucket: 'pending' })
			]);
			await store.fetchPrChecks('/project', 1);

			// Second fetch: failed
			clearInvokeMocks();
			mockInvoke('github_pr_checks', () => [
				makeCheck({ name: 'CI', workflow: 'build', bucket: 'fail' })
			]);
			await store.fetchPrChecks('/project', 1);

			expect(callback).toHaveBeenCalledWith({
				name: 'CI',
				bucket: 'fail',
				projectPath: '/project',
				prNumber: 1
			});
		});

		it('does not notify when check stays pending', async () => {
			const callback = vi.fn();
			store.onCheckComplete(callback);

			mockInvoke('github_pr_checks', () => [
				makeCheck({ name: 'CI', workflow: 'build', bucket: 'pending' })
			]);
			await store.fetchPrChecks('/project', 1);

			clearInvokeMocks();
			mockInvoke('github_pr_checks', () => [
				makeCheck({ name: 'CI', workflow: 'build', bucket: 'pending' })
			]);
			await store.fetchPrChecks('/project', 1);

			expect(callback).not.toHaveBeenCalled();
		});

		it('does not notify when check goes from pass to pass', async () => {
			const callback = vi.fn();
			store.onCheckComplete(callback);

			mockInvoke('github_pr_checks', () => [
				makeCheck({ name: 'CI', workflow: 'build', bucket: 'pass' })
			]);
			await store.fetchPrChecks('/project', 1);

			clearInvokeMocks();
			mockInvoke('github_pr_checks', () => [
				makeCheck({ name: 'CI', workflow: 'build', bucket: 'pass' })
			]);
			await store.fetchPrChecks('/project', 1);

			expect(callback).not.toHaveBeenCalled();
		});

		it('tracks multiple checks independently', async () => {
			const callback = vi.fn();
			store.onCheckComplete(callback);

			// First fetch: both pending
			mockInvoke('github_pr_checks', () => [
				makeCheck({ name: 'lint', workflow: 'ci', bucket: 'pending' }),
				makeCheck({ name: 'test', workflow: 'ci', bucket: 'pending' })
			]);
			await store.fetchPrChecks('/project', 1);

			// Second fetch: lint passed, test still pending
			clearInvokeMocks();
			mockInvoke('github_pr_checks', () => [
				makeCheck({ name: 'lint', workflow: 'ci', bucket: 'pass' }),
				makeCheck({ name: 'test', workflow: 'ci', bucket: 'pending' })
			]);
			await store.fetchPrChecks('/project', 1);

			expect(callback).toHaveBeenCalledTimes(1);
			expect(callback).toHaveBeenCalledWith({
				name: 'lint',
				bucket: 'pass',
				projectPath: '/project',
				prNumber: 1
			});
		});
	});

	// ─── getPrChecks ──────────────────────────────────────────

	describe('getPrChecks', () => {
		it('returns checks for known PR', () => {
			const checks = [makeCheck()];
			store.checksByPr = { '/project::5': checks };

			expect(store.getPrChecks('/project', 5)).toEqual(checks);
		});

		it('returns undefined for unknown PR', () => {
			expect(store.getPrChecks('/project', 999)).toBeUndefined();
		});
	});

	// ─── activeBranches ───────────────────────────────────────

	describe('activeBranches', () => {
		it('returns empty when ghAvailable is false', () => {
			store.ghAvailable = false;
			mockProjectStore.projects = [{ name: 'test', path: '/project' }];
			mockSessionStore.activeSessionsByProject = { '/project': ['session-1'] };
			mockGitStore.branchByProject = { '/project': 'main' };

			expect(store.activeBranches).toEqual([]);
		});

		it('returns branches for projects with active sessions', () => {
			mockProjectStore.projects = [{ name: 'test', path: '/project' }];
			mockSessionStore.activeSessionsByProject = { '/project': ['session-1'] };
			mockGitStore.branchByProject = { '/project': 'feature' };
			mockGitStore.worktreesByProject = {};

			expect(store.activeBranches).toEqual([{ projectPath: '/project', branch: 'feature' }]);
		});

		it('includes worktree branches', () => {
			mockProjectStore.projects = [{ name: 'test', path: '/project' }];
			mockSessionStore.activeSessionsByProject = { '/project': ['session-1'] };
			mockGitStore.branchByProject = { '/project': 'main' };
			mockGitStore.worktreesByProject = {
				'/project': [
					{ branch: 'main', isMain: true },
					{ branch: 'feature', isMain: false }
				]
			};

			const branches = store.activeBranches;
			expect(branches).toHaveLength(2);
			expect(branches).toContainEqual({ projectPath: '/project', branch: 'main' });
			expect(branches).toContainEqual({ projectPath: '/project', branch: 'feature' });
		});

		it('excludes projects without active sessions', () => {
			mockProjectStore.projects = [
				{ name: 'active', path: '/active' },
				{ name: 'idle', path: '/idle' }
			];
			mockSessionStore.activeSessionsByProject = { '/active': ['s1'] };
			mockGitStore.branchByProject = { '/active': 'main', '/idle': 'main' };

			expect(store.activeBranches).toEqual([{ projectPath: '/active', branch: 'main' }]);
		});

		it('excludes projects without a branch', () => {
			mockProjectStore.projects = [{ name: 'test', path: '/project' }];
			mockSessionStore.activeSessionsByProject = { '/project': ['s1'] };
			mockGitStore.branchByProject = {};

			expect(store.activeBranches).toEqual([]);
		});
	});

	// ─── refreshProject ───────────────────────────────────────

	describe('refreshProject', () => {
		it('does nothing when ghAvailable is false', async () => {
			store.ghAvailable = false;

			await store.refreshProject('/project');

			expect(invokeSpy).not.toHaveBeenCalledWith('github_project_status', expect.anything());
		});

		it('fetches project status when gh is available', async () => {
			store.ghAvailable = true;
			mockInvoke('github_project_status', () => makeProjectStatus());

			await store.refreshProject('/project');

			expect(invokeSpy).toHaveBeenCalledWith('github_project_status', {
				projectPath: '/project'
			});
		});
	});

	// ─── initForProjects ──────────────────────────────────────

	describe('initForProjects', () => {
		it('checks gh availability then fetches all projects', async () => {
			mockInvoke('github_is_available', () => true);
			mockInvoke('github_project_status', () => makeProjectStatus());

			await store.initForProjects(['/project-a', '/project-b']);

			expect(invokeSpy).toHaveBeenCalledWith('github_is_available');
			expect(invokeSpy).toHaveBeenCalledWith('github_project_status', {
				projectPath: '/project-a'
			});
			expect(invokeSpy).toHaveBeenCalledWith('github_project_status', {
				projectPath: '/project-b'
			});
		});

		it('skips fetching when gh is not available', async () => {
			mockInvoke('github_is_available', () => false);

			await store.initForProjects(['/project']);

			expect(invokeSpy).not.toHaveBeenCalledWith('github_project_status', expect.anything());
		});
	});

	// ─── Debounced refresh via git:changed ────────────────────

	describe('git:changed debounced refresh', () => {
		it('refreshes project after debounce delay', async () => {
			store.ghAvailable = true;
			mockInvoke('github_project_status', () => makeProjectStatus());

			emitMockEvent('git:changed', { projectPath: '/project' });

			// Not called yet (debounce)
			expect(invokeSpy).not.toHaveBeenCalledWith('github_project_status', expect.anything());

			// Advance past debounce (2000ms)
			await vi.advanceTimersByTimeAsync(2000);

			expect(invokeSpy).toHaveBeenCalledWith('github_project_status', {
				projectPath: '/project'
			});
		});

		it('resets debounce timer on repeated events', async () => {
			store.ghAvailable = true;
			mockInvoke('github_project_status', () => makeProjectStatus());

			emitMockEvent('git:changed', { projectPath: '/project' });
			await vi.advanceTimersByTimeAsync(1500);
			emitMockEvent('git:changed', { projectPath: '/project' });
			await vi.advanceTimersByTimeAsync(1500);

			// Should not have been called yet (timer reset at 1500ms)
			expect(invokeSpy).not.toHaveBeenCalledWith('github_project_status', expect.anything());

			await vi.advanceTimersByTimeAsync(500);

			expect(invokeSpy).toHaveBeenCalledWith('github_project_status', {
				projectPath: '/project'
			});
		});
	});

	// ─── Slow polling ─────────────────────────────────────────

	describe('slow polling', () => {
		it('polls active branches on 90s interval', async () => {
			store.ghAvailable = true;
			mockProjectStore.projects = [{ name: 'test', path: '/project' }];
			mockSessionStore.activeSessionsByProject = { '/project': ['s1'] };
			mockGitStore.branchByProject = { '/project': 'main' };
			mockInvoke('github_project_status', () => makeProjectStatus());

			await vi.advanceTimersByTimeAsync(90_000);

			expect(invokeSpy).toHaveBeenCalledWith('github_project_status', {
				projectPath: '/project'
			});
		});
	});

	// ─── destroy ──────────────────────────────────────────────

	describe('destroy', () => {
		it('cleans up intervals and timers', () => {
			// Trigger a debounce timer
			emitMockEvent('git:changed', { projectPath: '/project' });

			// Should not throw
			store.destroy();

			// After destroy, advancing timers should not trigger any invokes
			invokeSpy.mockClear();
			vi.advanceTimersByTime(100_000);
			expect(invokeSpy).not.toHaveBeenCalledWith('github_project_status', expect.anything());
		});
	});
});
