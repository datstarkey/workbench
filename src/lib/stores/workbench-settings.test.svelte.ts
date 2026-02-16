import { invokeSpy, mockInvoke, clearInvokeMocks } from '../../test/tauri-mocks';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { WorkbenchSettingsStore } from './workbench-settings.svelte';
import type { AgentAction, WorkbenchSettings } from '$types/workbench';

let uidCounter = 0;
vi.mock('$lib/utils/uid', () => ({
	uid: () => `uid-${++uidCounter}`
}));

function makeSettings(overrides: Partial<WorkbenchSettings> = {}): WorkbenchSettings {
	return {
		worktreeStrategy: 'sibling',
		agentActions: [],
		...overrides
	};
}

function makeAction(overrides: Partial<AgentAction> = {}): AgentAction {
	return {
		id: `action-${++uidCounter}`,
		name: 'Test Action',
		prompt: 'Do something useful',
		target: 'both',
		category: 'Testing',
		tags: ['test'],
		...overrides
	};
}

describe('WorkbenchSettingsStore', () => {
	let store: WorkbenchSettingsStore;

	beforeEach(() => {
		uidCounter = 0;
		store = new WorkbenchSettingsStore();
	});

	afterEach(() => {
		clearInvokeMocks();
	});

	// ─── load() ─────────────────────────────────────────────

	describe('load', () => {
		it('populates agentActions and worktreeStrategy', async () => {
			const actions = [makeAction({ id: 'a1', name: 'Review' })];
			mockInvoke('load_workbench_settings', () =>
				makeSettings({ worktreeStrategy: 'inside', agentActions: actions })
			);

			await store.load();

			expect(store.worktreeStrategy).toBe('inside');
			expect(store.agentActions).toHaveLength(1);
			expect(store.agentActions[0].name).toBe('Review');
		});

		it('sets loaded and clears dirty', async () => {
			mockInvoke('load_workbench_settings', () => makeSettings());
			store.dirty = true;

			await store.load();

			expect(store.loaded).toBe(true);
			expect(store.dirty).toBe(false);
		});

		it('normalizes missing fields with defaults', async () => {
			mockInvoke('load_workbench_settings', () => ({
				worktreeStrategy: 'sibling',
				agentActions: [
					{
						id: '',
						name: undefined,
						prompt: undefined,
						target: undefined,
						category: undefined,
						tags: undefined
					}
				]
			}));

			await store.load();

			const action = store.agentActions[0];
			expect(action.id).toBe('uid-1'); // generated from uid()
			expect(action.name).toBe('');
			expect(action.prompt).toBe('');
			expect(action.target).toBe('both');
			expect(action.category).toBe('');
			expect(action.tags).toEqual([]);
		});

		it('normalizes bad target to both', async () => {
			mockInvoke('load_workbench_settings', () =>
				makeSettings({
					agentActions: [makeAction({ target: 'invalid' as 'both' })]
				})
			);

			await store.load();

			expect(store.agentActions[0].target).toBe('both');
		});

		it('normalizes non-array tags to empty array', async () => {
			mockInvoke('load_workbench_settings', () =>
				makeSettings({
					agentActions: [makeAction({ tags: 'not-an-array' as unknown as string[] })]
				})
			);

			await store.load();

			expect(store.agentActions[0].tags).toEqual([]);
		});

		it('returns empty array when actions is not an array', async () => {
			mockInvoke('load_workbench_settings', () => ({
				worktreeStrategy: 'sibling',
				agentActions: 'corrupt'
			}));

			await store.load();

			expect(store.agentActions).toEqual([]);
		});

		it('preserves valid targets unchanged', async () => {
			mockInvoke('load_workbench_settings', () =>
				makeSettings({
					agentActions: [
						makeAction({ id: 'a1', target: 'claude' }),
						makeAction({ id: 'a2', target: 'codex' }),
						makeAction({ id: 'a3', target: 'both' })
					]
				})
			);

			await store.load();

			expect(store.agentActions[0].target).toBe('claude');
			expect(store.agentActions[1].target).toBe('codex');
			expect(store.agentActions[2].target).toBe('both');
		});

		it('trims and filters whitespace-only tags during normalization', async () => {
			mockInvoke('load_workbench_settings', () =>
				makeSettings({
					agentActions: [makeAction({ tags: ['  valid  ', '', '  ', 'ok'] })]
				})
			);

			await store.load();

			expect(store.agentActions[0].tags).toEqual(['valid', 'ok']);
		});

		it('normalizes multiple actions independently', async () => {
			mockInvoke('load_workbench_settings', () => ({
				worktreeStrategy: 'sibling',
				agentActions: [
					{ id: 'a1', name: 'Good', prompt: 'ok', target: 'claude', category: '', tags: [] },
					{
						id: '',
						name: undefined,
						prompt: undefined,
						target: 'bad',
						category: undefined,
						tags: null
					}
				]
			}));

			await store.load();

			expect(store.agentActions).toHaveLength(2);
			expect(store.agentActions[0].id).toBe('a1');
			expect(store.agentActions[0].target).toBe('claude');
			expect(store.agentActions[1].id).toBeTruthy(); // uid generated
			expect(store.agentActions[1].target).toBe('both');
			expect(store.agentActions[1].tags).toEqual([]);
		});
	});

	// ─── save() ─────────────────────────────────────────────

	describe('save', () => {
		it('sends correct payload via invoke', async () => {
			store.worktreeStrategy = 'inside';
			store.agentActions = [makeAction({ id: 'a1' })];

			await store.save();

			expect(invokeSpy).toHaveBeenCalledWith('save_workbench_settings', {
				settings: {
					worktreeStrategy: 'inside',
					agentActions: store.agentActions,
					claudeHooksApproved: null,
					codexConfigApproved: null
				}
			});
		});

		it('clears dirty flag on success', async () => {
			store.dirty = true;

			await store.save();

			expect(store.dirty).toBe(false);
		});

		it('manages saving flag', async () => {
			expect(store.saving).toBe(false);

			const savePromise = store.save();
			// saving should be true during the operation
			expect(store.saving).toBe(true);

			await savePromise;
			expect(store.saving).toBe(false);
		});

		it('clears saving flag even on failure', async () => {
			mockInvoke('save_workbench_settings', () => {
				throw new Error('save failed');
			});

			await expect(store.save()).rejects.toThrow('save failed');
			expect(store.saving).toBe(false);
		});
	});

	// ─── addAgentAction() ───────────────────────────────────

	describe('addAgentAction', () => {
		it('appends action with defaults', () => {
			store.addAgentAction();

			expect(store.agentActions).toHaveLength(1);
			const action = store.agentActions[0];
			expect(action.id).toBe('uid-1');
			expect(action.name).toBe('');
			expect(action.prompt).toBe('');
			expect(action.target).toBe('both');
			expect(action.category).toBe('');
			expect(action.tags).toEqual([]);
		});

		it('sets dirty flag', () => {
			expect(store.dirty).toBe(false);
			store.addAgentAction();
			expect(store.dirty).toBe(true);
		});

		it('preserves existing actions', () => {
			store.agentActions = [makeAction({ id: 'existing' })];

			store.addAgentAction();

			expect(store.agentActions).toHaveLength(2);
			expect(store.agentActions[0].id).toBe('existing');
		});

		it('generates unique IDs across multiple calls', () => {
			store.addAgentAction();
			store.addAgentAction();
			store.addAgentAction();

			const ids = store.agentActions.map((a) => a.id);
			expect(new Set(ids).size).toBe(3);
		});
	});

	// ─── updateAgentAction() ────────────────────────────────

	describe('updateAgentAction', () => {
		it('performs partial update by id', () => {
			store.agentActions = [makeAction({ id: 'a1', name: 'Old Name', prompt: 'Old Prompt' })];

			store.updateAgentAction('a1', { name: 'New Name' });

			expect(store.agentActions[0].name).toBe('New Name');
			expect(store.agentActions[0].prompt).toBe('Old Prompt'); // unchanged
		});

		it('sets dirty flag', () => {
			store.agentActions = [makeAction({ id: 'a1' })];

			store.updateAgentAction('a1', { name: 'Updated' });

			expect(store.dirty).toBe(true);
		});

		it('does not modify other actions', () => {
			store.agentActions = [
				makeAction({ id: 'a1', name: 'First' }),
				makeAction({ id: 'a2', name: 'Second' })
			];

			store.updateAgentAction('a1', { name: 'Updated' });

			expect(store.agentActions[1].name).toBe('Second');
		});

		it('updates multiple fields in a single call', () => {
			store.agentActions = [makeAction({ id: 'a1', name: 'Old', target: 'both', category: '' })];

			store.updateAgentAction('a1', { name: 'New', target: 'claude', category: 'Review' });

			expect(store.agentActions[0].name).toBe('New');
			expect(store.agentActions[0].target).toBe('claude');
			expect(store.agentActions[0].category).toBe('Review');
		});
	});

	// ─── removeAgentAction() ────────────────────────────────

	describe('removeAgentAction', () => {
		it('removes by id', () => {
			store.agentActions = [makeAction({ id: 'a1' }), makeAction({ id: 'a2' })];

			store.removeAgentAction('a1');

			expect(store.agentActions).toHaveLength(1);
			expect(store.agentActions[0].id).toBe('a2');
		});

		it('sets dirty flag', () => {
			store.agentActions = [makeAction({ id: 'a1' })];

			store.removeAgentAction('a1');

			expect(store.dirty).toBe(true);
		});

		it('no-ops for unknown id', () => {
			store.agentActions = [makeAction({ id: 'a1' })];

			store.removeAgentAction('nonexistent');

			expect(store.agentActions).toHaveLength(1);
		});
	});

	// ─── runnableActions derived ────────────────────────────

	describe('runnableActions', () => {
		it('filters out actions with empty name', () => {
			store.agentActions = [
				makeAction({ id: 'a1', name: '', prompt: 'Has prompt' }),
				makeAction({ id: 'a2', name: 'Has name', prompt: 'Has prompt' })
			];

			expect(store.runnableActions).toHaveLength(1);
			expect(store.runnableActions[0].id).toBe('a2');
		});

		it('filters out actions with empty prompt', () => {
			store.agentActions = [
				makeAction({ id: 'a1', name: 'Has name', prompt: '' }),
				makeAction({ id: 'a2', name: 'Has name', prompt: 'Has prompt' })
			];

			expect(store.runnableActions).toHaveLength(1);
			expect(store.runnableActions[0].id).toBe('a2');
		});

		it('filters out actions with whitespace-only name or prompt', () => {
			store.agentActions = [
				makeAction({ id: 'a1', name: '   ', prompt: 'Has prompt' }),
				makeAction({ id: 'a2', name: 'Has name', prompt: '   ' }),
				makeAction({ id: 'a3', name: 'Valid', prompt: 'Valid prompt' })
			];

			expect(store.runnableActions).toHaveLength(1);
			expect(store.runnableActions[0].id).toBe('a3');
		});

		it('trims name and prompt in output', () => {
			store.agentActions = [
				makeAction({ id: 'a1', name: '  Review PR  ', prompt: '  Check for bugs  ' })
			];

			expect(store.runnableActions[0].name).toBe('Review PR');
			expect(store.runnableActions[0].prompt).toBe('Check for bugs');
		});

		it('returns empty array when no actions configured', () => {
			store.agentActions = [];
			expect(store.runnableActions).toEqual([]);
		});

		it('preserves other fields unchanged', () => {
			store.agentActions = [
				makeAction({ id: 'a1', target: 'claude', category: 'Review', tags: ['pr'] })
			];

			const result = store.runnableActions[0];
			expect(result.target).toBe('claude');
			expect(result.category).toBe('Review');
			expect(result.tags).toEqual(['pr']);
		});

		it('reflects mutations to agentActions', () => {
			store.agentActions = [makeAction({ id: 'a1', name: 'First', prompt: 'p1' })];
			expect(store.runnableActions).toHaveLength(1);

			store.addAgentAction(); // new action with empty name/prompt
			expect(store.runnableActions).toHaveLength(1); // still filtered out

			store.updateAgentAction(store.agentActions[1].id, {
				name: 'Second',
				prompt: 'p2'
			});
			expect(store.runnableActions).toHaveLength(2);

			store.removeAgentAction('a1');
			expect(store.runnableActions).toHaveLength(1);
			expect(store.runnableActions[0].name).toBe('Second');
		});
	});

	// ─── load → save round-trip ─────────────────────────────

	describe('load → save round-trip', () => {
		it('save sends the same data that load populated', async () => {
			const actions = [makeAction({ id: 'a1', name: 'Review', target: 'claude', tags: ['pr'] })];
			mockInvoke('load_workbench_settings', () =>
				makeSettings({ worktreeStrategy: 'inside', agentActions: actions })
			);

			await store.load();
			invokeSpy.mockClear();
			await store.save();

			expect(invokeSpy).toHaveBeenCalledWith('save_workbench_settings', {
				settings: {
					worktreeStrategy: 'inside',
					agentActions: store.agentActions,
					claudeHooksApproved: null,
					codexConfigApproved: null
				}
			});
			expect(store.agentActions[0].name).toBe('Review');
			expect(store.agentActions[0].target).toBe('claude');
		});
	});

	// ─── setWorktreeStrategy() ──────────────────────────────

	describe('setWorktreeStrategy', () => {
		it('updates strategy and sets dirty', () => {
			store.setWorktreeStrategy('inside');

			expect(store.worktreeStrategy).toBe('inside');
			expect(store.dirty).toBe(true);
		});
	});
});
