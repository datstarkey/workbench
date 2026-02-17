import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invokeSpy, mockInvoke, clearInvokeMocks } from '../../test/tauri-mocks';
import { TrelloStore } from './trello.svelte';
import type { WorkspaceStore } from './workspaces.svelte';
import type { GitStore } from './git.svelte';
import type { GitHubStore } from './github.svelte';
import type { BoardConfig, TrelloBoardData } from '$types/trello';
import type { GitHubPR } from '$types/workbench';

// Mock stores
const mockWorkspaceStore = {
	activeWorkspace: null as { projectPath: string; worktreePath?: string; branch?: string } | null,
	resolvedBranch: vi.fn()
} as unknown as WorkspaceStore;

const mockGitStore = {} as unknown as GitStore;
const mockGitHubStore = {} as unknown as GitHubStore;

vi.mock('./context', () => ({
	getWorkspaceStore: () => mockWorkspaceStore,
	getGitStore: () => mockGitStore,
	getGitHubStore: () => mockGitHubStore
}));

describe('TrelloStore', () => {
	let store: TrelloStore;

	beforeEach(() => {
		(mockWorkspaceStore as unknown as Record<string, unknown>).activeWorkspace = null;
		vi.mocked(mockWorkspaceStore.resolvedBranch).mockReturnValue(undefined);
		store = new TrelloStore();
	});

	afterEach(() => {
		clearInvokeMocks();
	});

	// --- Credential lifecycle ---

	describe('loadCredentials', () => {
		it('sets authenticated to true when credentials exist', async () => {
			mockInvoke('trello_load_credentials', () => ({
				apiKey: 'key-1',
				token: 'tok-1'
			}));

			await store.loadCredentials();

			expect(store.credentials).toEqual({ apiKey: 'key-1', token: 'tok-1' });
			expect(store.authenticated).toBe(true);
		});

		it('sets authenticated to false when credentials are null', async () => {
			mockInvoke('trello_load_credentials', () => null);

			await store.loadCredentials();

			expect(store.credentials).toBeNull();
			expect(store.authenticated).toBe(false);
		});

		it('handles invoke failure gracefully', async () => {
			const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
			mockInvoke('trello_load_credentials', () => {
				throw new Error('file not found');
			});

			await store.loadCredentials();

			expect(store.credentials).toBeNull();
			expect(store.authenticated).toBe(false);
			expect(warnSpy).toHaveBeenCalledWith(
				'[TrelloStore] Failed to load credentials:',
				expect.any(Error)
			);
			warnSpy.mockRestore();
		});
	});

	describe('disconnect', () => {
		it('clears all state and invokes disconnect', async () => {
			// Pre-populate state
			store.credentials = { apiKey: 'k', token: 't' };
			store.authenticated = true;
			store.availableBoards = [{ id: 'b1', name: 'Board 1', url: '' }];
			store.boardDataCache = { key: { board: { id: 'b1', name: 'B', url: '' }, columns: [] } };

			await store.disconnect();

			expect(invokeSpy).toHaveBeenCalledWith('trello_disconnect');
			expect(store.credentials).toBeNull();
			expect(store.authenticated).toBe(false);
			expect(store.availableBoards).toEqual([]);
			expect(store.boardDataCache).toEqual({});
		});
	});

	// --- Task link/unlink ---

	describe('linkTaskToBranch', () => {
		it('adds a new task link and saves config', async () => {
			store.configByProject = {
				'/proj': { boards: [], taskLinks: [] }
			};

			store.linkTaskToBranch('/proj', 'card-1', 'board-1', 'feature-x');

			expect(invokeSpy).toHaveBeenCalledWith('trello_save_project_config', {
				projectPath: '/proj',
				config: {
					boards: [],
					taskLinks: [
						{
							cardId: 'card-1',
							boardId: 'board-1',
							branch: 'feature-x',
							worktreePath: undefined,
							projectPath: '/proj'
						}
					]
				}
			});
		});

		it('replaces existing link for the same card', async () => {
			store.configByProject = {
				'/proj': {
					boards: [],
					taskLinks: [
						{
							cardId: 'card-1',
							boardId: 'board-old',
							branch: 'old-branch',
							projectPath: '/proj'
						}
					]
				}
			};

			store.linkTaskToBranch('/proj', 'card-1', 'board-new', 'new-branch');

			expect(invokeSpy).toHaveBeenCalledWith('trello_save_project_config', {
				projectPath: '/proj',
				config: {
					boards: [],
					taskLinks: [
						{
							cardId: 'card-1',
							boardId: 'board-new',
							branch: 'new-branch',
							worktreePath: undefined,
							projectPath: '/proj'
						}
					]
				}
			});
		});

		it('creates config when none exists for project', async () => {
			store.configByProject = {};

			store.linkTaskToBranch('/proj', 'card-1', 'board-1', 'feat');

			expect(invokeSpy).toHaveBeenCalledWith('trello_save_project_config', {
				projectPath: '/proj',
				config: {
					boards: [],
					taskLinks: [
						{
							cardId: 'card-1',
							boardId: 'board-1',
							branch: 'feat',
							worktreePath: undefined,
							projectPath: '/proj'
						}
					]
				}
			});
		});

		it('executes link action when board has one configured', async () => {
			const linkAction = { moveToColumnId: 'col-wip', addLabelIds: [], removeLabelIds: [] };
			store.configByProject = {
				'/proj': {
					boards: [
						{
							boardId: 'board-1',
							boardName: 'B',
							hiddenColumns: [],
							linkAction
						}
					],
					taskLinks: []
				}
			};

			store.linkTaskToBranch('/proj', 'card-1', 'board-1', 'feat');

			// Should invoke move_card for the link action
			expect(invokeSpy).toHaveBeenCalledWith('trello_move_card', {
				cardId: 'card-1',
				targetListId: 'col-wip'
			});
		});
	});

	describe('unlinkTask', () => {
		it('removes the task link and saves config', async () => {
			store.configByProject = {
				'/proj': {
					boards: [],
					taskLinks: [
						{ cardId: 'card-1', boardId: 'b', branch: 'x', projectPath: '/proj' },
						{ cardId: 'card-2', boardId: 'b', branch: 'y', projectPath: '/proj' }
					]
				}
			};

			store.unlinkTask('/proj', 'card-1');

			expect(invokeSpy).toHaveBeenCalledWith('trello_save_project_config', {
				projectPath: '/proj',
				config: {
					boards: [],
					taskLinks: [{ cardId: 'card-2', boardId: 'b', branch: 'y', projectPath: '/proj' }]
				}
			});
		});

		it('no-ops when project config does not exist', () => {
			store.configByProject = {};

			store.unlinkTask('/proj', 'card-1');

			expect(invokeSpy).not.toHaveBeenCalled();
		});
	});

	// --- Merge detection ---

	describe('checkForMergedPrs', () => {
		it('executes merge action on OPEN -> MERGED transition', async () => {
			const mergeAction = {
				moveToColumnId: 'col-done',
				addLabelIds: ['label-shipped'],
				removeLabelIds: []
			};
			store.configByProject = {
				'/proj': {
					boards: [
						{
							boardId: 'board-1',
							boardName: 'B',
							hiddenColumns: [],
							mergeAction
						}
					],
					taskLinks: [
						{ cardId: 'card-1', boardId: 'board-1', branch: 'feat-x', projectPath: '/proj' }
					]
				}
			};

			const pr: GitHubPR = {
				number: 1,
				title: 'PR',
				state: 'OPEN',
				url: '',
				isDraft: false,
				headRefName: 'feat-x',
				reviewDecision: null,
				checksStatus: { overall: 'success', total: 1, passing: 1, failing: 0, pending: 0 },
				mergeStateStatus: 'CLEAN'
			};

			// First call: records initial state
			store.checkForMergedPrs({ '/proj': [pr] });
			expect(invokeSpy).not.toHaveBeenCalledWith('trello_move_card', expect.anything());

			// Second call: OPEN -> MERGED
			invokeSpy.mockClear();
			const mergedPr: GitHubPR = { ...pr, state: 'MERGED' };
			store.checkForMergedPrs({ '/proj': [mergedPr] });

			// executeBoardAction is async (fire-and-forget), flush microtasks
			await new Promise((r) => setTimeout(r, 0));

			expect(invokeSpy).toHaveBeenCalledWith('trello_move_card', {
				cardId: 'card-1',
				targetListId: 'col-done'
			});
			expect(invokeSpy).toHaveBeenCalledWith('trello_add_label', {
				cardId: 'card-1',
				labelId: 'label-shipped'
			});
		});

		it('does not trigger action when no task link exists', () => {
			store.configByProject = {
				'/proj': {
					boards: [
						{
							boardId: 'board-1',
							boardName: 'B',
							hiddenColumns: [],
							mergeAction: { moveToColumnId: 'col-done', addLabelIds: [], removeLabelIds: [] }
						}
					],
					taskLinks: [] // no links
				}
			};

			const pr: GitHubPR = {
				number: 1,
				title: 'PR',
				state: 'OPEN',
				url: '',
				isDraft: false,
				headRefName: 'feat-x',
				reviewDecision: null,
				checksStatus: { overall: 'success', total: 1, passing: 1, failing: 0, pending: 0 },
				mergeStateStatus: 'CLEAN'
			};

			store.checkForMergedPrs({ '/proj': [pr] });
			const mergedPr: GitHubPR = { ...pr, state: 'MERGED' };
			invokeSpy.mockClear();
			store.checkForMergedPrs({ '/proj': [mergedPr] });

			expect(invokeSpy).not.toHaveBeenCalledWith('trello_move_card', expect.anything());
		});

		it('does not trigger on first observation (no previous state)', () => {
			store.configByProject = {
				'/proj': {
					boards: [
						{
							boardId: 'board-1',
							boardName: 'B',
							hiddenColumns: [],
							mergeAction: { moveToColumnId: 'done', addLabelIds: [], removeLabelIds: [] }
						}
					],
					taskLinks: [
						{ cardId: 'card-1', boardId: 'board-1', branch: 'feat', projectPath: '/proj' }
					]
				}
			};

			const pr: GitHubPR = {
				number: 1,
				title: 'PR',
				state: 'MERGED',
				url: '',
				isDraft: false,
				headRefName: 'feat',
				reviewDecision: null,
				checksStatus: { overall: 'success', total: 1, passing: 1, failing: 0, pending: 0 },
				mergeStateStatus: 'CLEAN'
			};

			// First observation — MERGED but no previous state
			store.checkForMergedPrs({ '/proj': [pr] });

			expect(invokeSpy).not.toHaveBeenCalledWith('trello_move_card', expect.anything());
		});

		it('skips projects without config', () => {
			store.configByProject = {};

			const pr: GitHubPR = {
				number: 1,
				title: 'PR',
				state: 'OPEN',
				url: '',
				isDraft: false,
				headRefName: 'feat',
				reviewDecision: null,
				checksStatus: { overall: 'success', total: 1, passing: 1, failing: 0, pending: 0 },
				mergeStateStatus: 'CLEAN'
			};

			// Should not throw
			store.checkForMergedPrs({ '/proj': [pr] });
			expect(invokeSpy).not.toHaveBeenCalled();
		});
	});

	// --- Derived state ---

	describe('derived: activeBoards', () => {
		it('returns boards for active project', () => {
			const boards: BoardConfig[] = [{ boardId: 'b1', boardName: 'Board 1', hiddenColumns: [] }];
			store.configByProject = { '/proj': { boards, taskLinks: [] } };
			(mockWorkspaceStore as unknown as Record<string, unknown>).activeWorkspace = {
				projectPath: '/proj'
			};

			expect(store.activeBoards).toEqual(boards);
		});

		it('returns empty array when no active workspace', () => {
			(mockWorkspaceStore as unknown as Record<string, unknown>).activeWorkspace = null;

			expect(store.activeBoards).toEqual([]);
		});
	});

	describe('derived: linkedTask', () => {
		it('returns task link matching active branch', () => {
			const link = {
				cardId: 'card-1',
				boardId: 'b1',
				branch: 'feat-x',
				projectPath: '/proj'
			};
			store.configByProject = { '/proj': { boards: [], taskLinks: [link] } };
			(mockWorkspaceStore as unknown as Record<string, unknown>).activeWorkspace = {
				projectPath: '/proj'
			};
			vi.mocked(mockWorkspaceStore.resolvedBranch).mockReturnValue('feat-x');

			expect(store.linkedTask).toEqual(link);
		});

		it('returns null when no branch matches', () => {
			store.configByProject = {
				'/proj': {
					boards: [],
					taskLinks: [{ cardId: 'card-1', boardId: 'b1', branch: 'other', projectPath: '/proj' }]
				}
			};
			(mockWorkspaceStore as unknown as Record<string, unknown>).activeWorkspace = {
				projectPath: '/proj'
			};
			vi.mocked(mockWorkspaceStore.resolvedBranch).mockReturnValue('feat-x');

			expect(store.linkedTask).toBeNull();
		});
	});

	describe('derived: activeBoardData', () => {
		it('returns board data for active boards', () => {
			const boardData: TrelloBoardData = {
				board: { id: 'b1', name: 'Board 1', url: '' },
				columns: []
			};
			store.configByProject = {
				'/proj': {
					boards: [{ boardId: 'b1', boardName: 'Board 1', hiddenColumns: [] }],
					taskLinks: []
				}
			};
			store.boardDataCache = { '/proj::b1': boardData };
			(mockWorkspaceStore as unknown as Record<string, unknown>).activeWorkspace = {
				projectPath: '/proj'
			};

			expect(store.activeBoardData).toEqual([boardData]);
		});

		it('filters out boards without cached data', () => {
			store.configByProject = {
				'/proj': {
					boards: [
						{ boardId: 'b1', boardName: 'Board 1', hiddenColumns: [] },
						{ boardId: 'b2', boardName: 'Board 2', hiddenColumns: [] }
					],
					taskLinks: []
				}
			};
			const boardData: TrelloBoardData = {
				board: { id: 'b1', name: 'Board 1', url: '' },
				columns: []
			};
			store.boardDataCache = { '/proj::b1': boardData };
			(mockWorkspaceStore as unknown as Record<string, unknown>).activeWorkspace = {
				projectPath: '/proj'
			};

			expect(store.activeBoardData).toEqual([boardData]);
		});
	});

	// --- Sidebar tab ---

	describe('setSidebarTab', () => {
		it('updates sidebarTab state', () => {
			expect(store.sidebarTab).toBe('github');

			store.setSidebarTab('boards');

			expect(store.sidebarTab).toBe('boards');
		});
	});
});
