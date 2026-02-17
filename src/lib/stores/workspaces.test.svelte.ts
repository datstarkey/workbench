import { invokeSpy, clearInvokeMocks } from '../../test/tauri-mocks';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { WorkspaceStore } from './workspaces.svelte';
import type { ProjectConfig, ProjectWorkspace, TerminalTabState } from '$types/workbench';

// Mock uid to produce predictable IDs
let uidCounter = 0;
vi.mock('$lib/utils/uid', () => ({
	uid: () => `uid-${++uidCounter}`
}));

// Mock context so getGitStore() works outside a component
const mockGitStore = {
	watchProject: vi.fn(),
	unwatchProject: vi.fn(),
	branchByProject: {} as Record<string, string>
};
vi.mock('./context', () => ({
	getGitStore: () => mockGitStore
}));

// Helper factories

function makeProject(overrides: Partial<ProjectConfig> = {}): ProjectConfig {
	return { name: 'Test Project', path: '/projects/test', ...overrides };
}

function makeWorkspace(overrides: Partial<ProjectWorkspace> = {}): ProjectWorkspace {
	return {
		id: `ws-${++uidCounter}`,
		projectPath: '/projects/test',
		projectName: 'Test Project',
		terminalTabs: [],
		activeTerminalTabId: '',
		...overrides
	};
}

function makeTab(overrides: Partial<TerminalTabState> = {}): TerminalTabState {
	return {
		id: `tab-${++uidCounter}`,
		label: 'Terminal 1',
		split: 'horizontal',
		panes: [{ id: `pane-${++uidCounter}` }],
		...overrides
	};
}

describe('WorkspaceStore', () => {
	let store: WorkspaceStore;

	beforeEach(() => {
		uidCounter = 0;
		mockGitStore.branchByProject = {};
		store = new WorkspaceStore();
	});

	afterEach(() => {
		clearInvokeMocks();
	});

	// ─── Reactive Getters ───────────────────────────────────

	describe('activeWorkspaceId', () => {
		it('returns null when no workspaces exist', () => {
			expect(store.activeWorkspaceId).toBeNull();
		});

		it('returns selectedId when it matches a workspace', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];
			store.selectedId = 'ws-a';
			expect(store.activeWorkspaceId).toBe('ws-a');
		});

		it('falls back to first workspace when selectedId is invalid', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];
			store.selectedId = 'nonexistent';
			expect(store.activeWorkspaceId).toBe('ws-a');
		});

		it('falls back to first workspace when selectedId is null', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];
			store.selectedId = null;
			expect(store.activeWorkspaceId).toBe('ws-a');
		});
	});

	describe('activeWorkspace', () => {
		it('returns null when no workspaces exist', () => {
			expect(store.activeWorkspace).toBeNull();
		});

		it('returns the workspace matching activeWorkspaceId', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];
			store.selectedId = 'ws-a';
			expect(store.activeWorkspace).toEqual(ws);
		});
	});

	describe('activeTerminalTab', () => {
		it('returns null when no active workspace', () => {
			expect(store.activeTerminalTab).toBeNull();
		});

		it('returns the tab matching activeTerminalTabId', () => {
			const tab = makeTab({ id: 'tab-1' });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];
			store.selectedId = 'ws-a';
			expect(store.activeTerminalTab).toEqual(tab);
		});

		it('falls back to first tab when activeTerminalTabId is stale', () => {
			const tab1 = makeTab({ id: 'tab-1' });
			const tab2 = makeTab({ id: 'tab-2' });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab1, tab2],
				activeTerminalTabId: 'nonexistent'
			});
			store.workspaces = [ws];
			store.selectedId = 'ws-a';
			expect(store.activeTerminalTab).toEqual(tab1);
		});

		it('returns null when workspace has no tabs', () => {
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [],
				activeTerminalTabId: ''
			});
			store.workspaces = [ws];
			store.selectedId = 'ws-a';
			expect(store.activeTerminalTab).toBeNull();
		});
	});

	describe('openProjectPaths', () => {
		it('returns all project paths', () => {
			store.workspaces = [
				makeWorkspace({ projectPath: '/a' }),
				makeWorkspace({ projectPath: '/b' })
			];
			expect(store.openProjectPaths).toEqual(['/a', '/b']);
		});
	});

	describe('activeProjectPath', () => {
		it('returns null when no active workspace', () => {
			expect(store.activeProjectPath).toBeNull();
		});

		it('returns active workspace project path', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a', projectPath: '/projects/foo' })];
			store.selectedId = 'ws-a';
			expect(store.activeProjectPath).toBe('/projects/foo');
		});
	});

	// ─── Lookup Methods ─────────────────────────────────────

	describe('getByProjectPath', () => {
		it('finds main workspace (no worktreePath)', () => {
			const main = makeWorkspace({ projectPath: '/a' });
			const wt = makeWorkspace({ projectPath: '/a', worktreePath: '/a-wt' });
			store.workspaces = [main, wt];
			expect(store.getByProjectPath('/a')).toEqual(main);
		});

		it('returns undefined for unknown path', () => {
			expect(store.getByProjectPath('/unknown')).toBeUndefined();
		});
	});

	describe('getByWorktreePath', () => {
		it('finds worktree workspace', () => {
			const wt = makeWorkspace({ projectPath: '/a', worktreePath: '/a-wt' });
			store.workspaces = [wt];
			expect(store.getByWorktreePath('/a-wt')).toEqual(wt);
		});

		it('returns undefined for unknown worktree path', () => {
			expect(store.getByWorktreePath('/unknown')).toBeUndefined();
		});
	});

	describe('getWorkspacesForProject', () => {
		it('returns all workspaces for a project', () => {
			const main = makeWorkspace({ id: 'main', projectPath: '/a' });
			const wt = makeWorkspace({ id: 'wt', projectPath: '/a', worktreePath: '/a-wt' });
			const other = makeWorkspace({ id: 'other', projectPath: '/b' });
			store.workspaces = [main, wt, other];
			expect(store.getWorkspacesForProject('/a')).toEqual([main, wt]);
		});
	});

	// ─── Workspace CRUD ─────────────────────────────────────

	describe('open', () => {
		it('creates a new workspace and selects it', () => {
			const project = makeProject({ path: '/projects/new', name: 'New' });
			store.open(project);

			expect(store.workspaces).toHaveLength(1);
			expect(store.workspaces[0].projectPath).toBe('/projects/new');
			expect(store.workspaces[0].projectName).toBe('New');
			expect(store.selectedId).toBe(store.workspaces[0].id);
			expect(invokeSpy).toHaveBeenCalledWith('save_workspaces', expect.any(Object));
		});

		it('selects existing workspace for same project', () => {
			const existing = makeWorkspace({ id: 'existing', projectPath: '/a' });
			store.workspaces = [existing];
			store.selectedId = null;

			store.open(makeProject({ path: '/a' }));

			expect(store.workspaces).toHaveLength(1);
			expect(store.selectedId).toBe('existing');
		});

		it('does not create duplicate workspace for same project', () => {
			const existing = makeWorkspace({ id: 'existing', projectPath: '/a' });
			store.workspaces = [existing];

			store.open(makeProject({ path: '/a' }));
			store.open(makeProject({ path: '/a' }));

			expect(store.workspaces).toHaveLength(1);
		});
	});

	describe('openWorktree', () => {
		it('creates a worktree workspace', () => {
			const project = makeProject({ path: '/projects/main' });
			store.openWorktree(project, '/projects/main-wt', 'feature-branch');

			expect(store.workspaces).toHaveLength(1);
			expect(store.workspaces[0].worktreePath).toBe('/projects/main-wt');
			expect(store.workspaces[0].branch).toBe('feature-branch');
			expect(store.workspaces[0].projectPath).toBe('/projects/main');
			expect(store.selectedId).toBe(store.workspaces[0].id);
		});

		it('selects existing worktree workspace', () => {
			const existing = makeWorkspace({
				id: 'wt-existing',
				projectPath: '/a',
				worktreePath: '/a-wt'
			});
			store.workspaces = [existing];

			store.openWorktree(makeProject({ path: '/a' }), '/a-wt', 'branch');

			expect(store.workspaces).toHaveLength(1);
			expect(store.selectedId).toBe('wt-existing');
		});
	});

	describe('close', () => {
		it('removes the workspace', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];
			store.selectedId = 'ws-a';

			store.close('ws-a');

			expect(store.workspaces).toHaveLength(0);
			expect(store.selectedId).toBeNull();
		});

		it('selects next workspace when closing selected', () => {
			const ws1 = makeWorkspace({ id: 'ws-1' });
			const ws2 = makeWorkspace({ id: 'ws-2' });
			const ws3 = makeWorkspace({ id: 'ws-3' });
			store.workspaces = [ws1, ws2, ws3];
			store.selectedId = 'ws-2';

			store.close('ws-2');

			// After removing ws-2 at index 1, ws-3 is now at index 1, so it becomes fallback
			expect(store.selectedId).toBe('ws-3');
		});

		it('selects previous workspace when closing last item', () => {
			const ws1 = makeWorkspace({ id: 'ws-1' });
			const ws2 = makeWorkspace({ id: 'ws-2' });
			store.workspaces = [ws1, ws2];
			store.selectedId = 'ws-2';

			store.close('ws-2');

			expect(store.selectedId).toBe('ws-1');
		});

		it('does not change selectedId when closing non-selected workspace', () => {
			const ws1 = makeWorkspace({ id: 'ws-1' });
			const ws2 = makeWorkspace({ id: 'ws-2' });
			store.workspaces = [ws1, ws2];
			store.selectedId = 'ws-1';

			store.close('ws-2');

			expect(store.selectedId).toBe('ws-1');
			expect(store.workspaces).toHaveLength(1);
		});

		it('no-ops for unknown workspace id', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];
			store.selectedId = 'ws-a';

			store.close('nonexistent');

			expect(store.workspaces).toHaveLength(1);
		});
	});

	describe('closeAllForProject', () => {
		it('removes main and worktree workspaces for a project', () => {
			const main = makeWorkspace({ id: 'main', projectPath: '/a' });
			const wt = makeWorkspace({ id: 'wt', projectPath: '/a', worktreePath: '/a-wt' });
			const other = makeWorkspace({ id: 'other', projectPath: '/b' });
			store.workspaces = [main, wt, other];
			store.selectedId = 'main';

			store.closeAllForProject('/a');

			expect(store.workspaces).toHaveLength(1);
			expect(store.workspaces[0].id).toBe('other');
			expect(store.selectedId).toBe('other');
		});

		it('no-ops when project has no workspaces', () => {
			const ws = makeWorkspace({ projectPath: '/a' });
			store.workspaces = [ws];

			store.closeAllForProject('/nonexistent');

			expect(store.workspaces).toHaveLength(1);
		});

		it('sets selectedId to null when all workspaces removed', () => {
			const ws = makeWorkspace({ id: 'ws-a', projectPath: '/a' });
			store.workspaces = [ws];
			store.selectedId = 'ws-a';

			store.closeAllForProject('/a');

			expect(store.workspaces).toHaveLength(0);
			expect(store.selectedId).toBeNull();
		});

		it('preserves selectedId when selected workspace is from another project', () => {
			const a = makeWorkspace({ id: 'a', projectPath: '/a' });
			const b = makeWorkspace({ id: 'b', projectPath: '/b' });
			store.workspaces = [a, b];
			store.selectedId = 'b';

			store.closeAllForProject('/a');

			expect(store.selectedId).toBe('b');
		});
	});

	describe('reorder', () => {
		it('moves workspace from one position to another', () => {
			const ws1 = makeWorkspace({ id: 'ws-1' });
			const ws2 = makeWorkspace({ id: 'ws-2' });
			const ws3 = makeWorkspace({ id: 'ws-3' });
			store.workspaces = [ws1, ws2, ws3];

			store.reorder('ws-1', 'ws-3');

			expect(store.workspaces.map((w) => w.id)).toEqual(['ws-2', 'ws-3', 'ws-1']);
		});

		it('no-ops when from and to are the same', () => {
			const ws1 = makeWorkspace({ id: 'ws-1' });
			store.workspaces = [ws1];

			store.reorder('ws-1', 'ws-1');

			expect(store.workspaces).toHaveLength(1);
		});

		it('no-ops for unknown ids', () => {
			const ws1 = makeWorkspace({ id: 'ws-1' });
			store.workspaces = [ws1];

			store.reorder('ws-1', 'nonexistent');

			expect(store.workspaces).toHaveLength(1);
		});
	});

	describe('updateProjectInfo', () => {
		it('updates projectPath and projectName for matching workspaces', () => {
			store.workspaces = [
				makeWorkspace({ id: 'ws-a', projectPath: '/old', projectName: 'Old' }),
				makeWorkspace({ id: 'ws-b', projectPath: '/other', projectName: 'Other' })
			];

			store.updateProjectInfo('/old', '/new', 'New');

			expect(store.workspaces[0].projectPath).toBe('/new');
			expect(store.workspaces[0].projectName).toBe('New');
			expect(store.workspaces[1].projectPath).toBe('/other');
		});
	});

	// ─── Terminal Tab Operations ────────────────────────────

	describe('addTerminalTab', () => {
		it('adds a terminal tab and selects it', () => {
			const ws = makeWorkspace({ id: 'ws-a', terminalTabs: [] });
			store.workspaces = [ws];

			store.addTerminalTab('ws-a', makeProject());

			const updated = store.workspaces[0];
			expect(updated.terminalTabs).toHaveLength(1);
			expect(updated.terminalTabs[0].label).toBe('Terminal 1');
			expect(updated.activeTerminalTabId).toBe(updated.terminalTabs[0].id);
		});

		it('increments tab label based on existing count', () => {
			const existingTab = makeTab({ label: 'Terminal 1' });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [existingTab],
				activeTerminalTabId: existingTab.id
			});
			store.workspaces = [ws];

			store.addTerminalTab('ws-a', makeProject());

			expect(store.workspaces[0].terminalTabs).toHaveLength(2);
			expect(store.workspaces[0].terminalTabs[1].label).toBe('Terminal 2');
		});

		it('does not include startup command (not first for project)', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];

			store.addTerminalTab('ws-a', makeProject({ startupCommand: 'echo hello' }));

			const pane = store.workspaces[0].terminalTabs[0].panes[0];
			expect(pane.startupCommand).toBeUndefined();
		});

		it('persists after adding', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			invokeSpy.mockClear();

			store.addTerminalTab('ws-a', makeProject());

			expect(invokeSpy).toHaveBeenCalledWith('save_workspaces', expect.any(Object));
		});
	});

	describe('addProjectTaskTab', () => {
		it('creates a tab with the task name and command', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];

			const result = store.addProjectTaskTab('ws-a', { name: 'Build', command: 'npm run build' });

			const updated = store.workspaces[0];
			expect(updated.terminalTabs).toHaveLength(1);
			expect(updated.terminalTabs[0].label).toBe('Build');
			expect(updated.terminalTabs[0].panes[0].startupCommand).toBe('npm run build');
			expect(updated.activeTerminalTabId).toBe(updated.terminalTabs[0].id);
			expect(result.tabId).toBe(updated.terminalTabs[0].id);
		});
	});

	describe('closeTerminalTab', () => {
		it('removes the tab', () => {
			const tab1 = makeTab({ id: 'tab-1' });
			const tab2 = makeTab({ id: 'tab-2' });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab1, tab2],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.closeTerminalTab('ws-a', 'tab-1');

			expect(store.workspaces[0].terminalTabs).toHaveLength(1);
			expect(store.workspaces[0].terminalTabs[0].id).toBe('tab-2');
		});

		it('selects next tab when closing active tab', () => {
			const tab1 = makeTab({ id: 'tab-1' });
			const tab2 = makeTab({ id: 'tab-2' });
			const tab3 = makeTab({ id: 'tab-3' });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab1, tab2, tab3],
				activeTerminalTabId: 'tab-2'
			});
			store.workspaces = [ws];

			store.closeTerminalTab('ws-a', 'tab-2');

			// tab-2 was at index 1. After removal, tab-3 is at index 1 -> fallback
			expect(store.workspaces[0].activeTerminalTabId).toBe('tab-3');
		});

		it('selects previous tab when closing last active tab', () => {
			const tab1 = makeTab({ id: 'tab-1' });
			const tab2 = makeTab({ id: 'tab-2' });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab1, tab2],
				activeTerminalTabId: 'tab-2'
			});
			store.workspaces = [ws];

			store.closeTerminalTab('ws-a', 'tab-2');

			expect(store.workspaces[0].activeTerminalTabId).toBe('tab-1');
		});

		it('preserves activeTerminalTabId when closing non-active tab', () => {
			const tab1 = makeTab({ id: 'tab-1' });
			const tab2 = makeTab({ id: 'tab-2' });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab1, tab2],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.closeTerminalTab('ws-a', 'tab-2');

			expect(store.workspaces[0].activeTerminalTabId).toBe('tab-1');
		});

		it('sets empty activeTerminalTabId when last tab removed', () => {
			const tab = makeTab({ id: 'tab-1' });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.closeTerminalTab('ws-a', 'tab-1');

			expect(store.workspaces[0].terminalTabs).toHaveLength(0);
			expect(store.workspaces[0].activeTerminalTabId).toBe('');
		});
	});

	describe('setActiveTab', () => {
		it('updates the active terminal tab id', () => {
			const tab1 = makeTab({ id: 'tab-1' });
			const tab2 = makeTab({ id: 'tab-2' });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab1, tab2],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.setActiveTab('ws-a', 'tab-2');

			expect(store.workspaces[0].activeTerminalTabId).toBe('tab-2');
		});
	});

	describe('splitTerminal', () => {
		it('adds a pane to the active tab', () => {
			const tab = makeTab({ id: 'tab-1', panes: [{ id: 'pane-1' }] });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.splitTerminal('ws-a', 'vertical');

			const updatedTab = store.workspaces[0].terminalTabs[0];
			expect(updatedTab.panes).toHaveLength(2);
			expect(updatedTab.split).toBe('vertical');
		});

		it('sets the split direction', () => {
			const tab = makeTab({ id: 'tab-1', split: 'horizontal', panes: [{ id: 'pane-1' }] });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.splitTerminal('ws-a', 'vertical');

			expect(store.workspaces[0].terminalTabs[0].split).toBe('vertical');
		});

		it('no-ops when no active tab found', () => {
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [],
				activeTerminalTabId: 'nonexistent'
			});
			store.workspaces = [ws];

			store.splitTerminal('ws-a', 'horizontal');

			expect(store.workspaces[0].terminalTabs).toHaveLength(0);
		});
	});

	describe('removePane', () => {
		it('removes a pane from the active tab', () => {
			const tab = makeTab({
				id: 'tab-1',
				panes: [{ id: 'pane-1' }, { id: 'pane-2' }]
			});
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.removePane('ws-a', 'pane-1');

			expect(store.workspaces[0].terminalTabs[0].panes).toHaveLength(1);
			expect(store.workspaces[0].terminalTabs[0].panes[0].id).toBe('pane-2');
		});

		it('does not remove last pane', () => {
			const tab = makeTab({ id: 'tab-1', panes: [{ id: 'pane-1' }] });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.removePane('ws-a', 'pane-1');

			expect(store.workspaces[0].terminalTabs[0].panes).toHaveLength(1);
			expect(store.workspaces[0].terminalTabs[0].panes[0].id).toBe('pane-1');
		});

		it('no-ops when no active tab', () => {
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [],
				activeTerminalTabId: 'nonexistent'
			});
			store.workspaces = [ws];

			store.removePane('ws-a', 'pane-1');

			expect(store.workspaces[0].terminalTabs).toHaveLength(0);
		});
	});

	// ─── AI Session Operations ──────────────────────────────

	describe('addAISession', () => {
		it('creates a claude tab with correct type', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];

			const { tabId } = store.addAISession('ws-a', 'claude');

			const updated = store.workspaces[0];
			expect(updated.terminalTabs).toHaveLength(1);
			const tab = updated.terminalTabs[0];
			expect(tab.type).toBe('claude');
			expect(tab.label).toBe('Claude 1');
			expect(tab.panes[0].type).toBe('claude');
			expect(tab.panes[0].startupCommand).toBe('claude');
			expect(tab.panes[0].claudeSessionId).toBe('');
			expect(updated.activeTerminalTabId).toBe(tabId);
		});

		it('creates a codex tab with correct type', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];

			store.addAISession('ws-a', 'codex');

			const tab = store.workspaces[0].terminalTabs[0];
			expect(tab.type).toBe('codex');
			expect(tab.label).toBe('Codex 1');
			expect(tab.panes[0].type).toBe('codex');
			expect(tab.panes[0].startupCommand).toBe('codex');
		});

		it('defaults to claude type', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];

			store.addAISession('ws-a');

			expect(store.workspaces[0].terminalTabs[0].type).toBe('claude');
		});

		it('increments label based on existing sessions of the same type', () => {
			const existingTab = makeTab({ id: 'existing', type: 'claude' });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [existingTab],
				activeTerminalTabId: 'existing'
			});
			store.workspaces = [ws];

			store.addAISession('ws-a', 'claude');

			expect(store.workspaces[0].terminalTabs[1].label).toBe('Claude 2');
		});

		it('returns the tab id', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];

			const { tabId } = store.addAISession('ws-a');

			expect(tabId).toBeTruthy();
			expect(store.workspaces[0].terminalTabs[0].id).toBe(tabId);
		});

		it('accepts explicit label and startup command options', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];

			store.addAISession('ws-a', 'claude', {
				label: 'Review PR',
				startupCommand: "claude 'Review this PR for regressions'"
			});

			const tab = store.workspaces[0].terminalTabs[0];
			expect(tab.label).toBe('Review PR');
			expect(tab.panes[0].startupCommand).toBe("claude 'Review this PR for regressions'");
		});
	});

	describe('addClaudeSession', () => {
		it('delegates to addAISession with claude type', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];

			const { tabId } = store.addClaudeSession('ws-a');

			expect(store.workspaces[0].terminalTabs[0].type).toBe('claude');
			expect(tabId).toBeTruthy();
		});
	});

	describe('resumeAISession', () => {
		it('creates a tab with resume command for claude', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];
			const sessionId = '12345678-1234-1234-1234-123456789abc';

			store.resumeAISession('ws-a', sessionId, 'My Session', 'claude');

			const tab = store.workspaces[0].terminalTabs[0];
			expect(tab.label).toBe('My Session');
			expect(tab.type).toBe('claude');
			expect(tab.panes[0].claudeSessionId).toBe(sessionId);
			expect(tab.panes[0].startupCommand).toBe(`claude --resume ${sessionId}`);
			expect(store.workspaces[0].activeTerminalTabId).toBe(tab.id);
		});

		it('creates a tab with resume command for codex', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];
			const sessionId = '12345678-1234-1234-1234-123456789abc';

			store.resumeAISession('ws-a', sessionId, 'My Codex', 'codex');

			const tab = store.workspaces[0].terminalTabs[0];
			expect(tab.type).toBe('codex');
			expect(tab.panes[0].startupCommand).toBe(`codex resume ${sessionId}`);
		});
	});

	describe('restartAISession', () => {
		it('replaces the tab with a new one preserving session ID', () => {
			const sessionId = '12345678-1234-1234-1234-123456789abc';
			const tab = makeTab({
				id: 'tab-1',
				type: 'claude',
				panes: [{ id: 'pane-1', type: 'claude', claudeSessionId: sessionId }]
			});
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.restartAISession('ws-a', 'tab-1');

			const updated = store.workspaces[0];
			expect(updated.terminalTabs).toHaveLength(1);
			// New tab replaces old one — new id
			expect(updated.terminalTabs[0].id).not.toBe('tab-1');
			expect(updated.terminalTabs[0].type).toBe('claude');
			expect(updated.terminalTabs[0].panes[0].claudeSessionId).toBe(sessionId);
			expect(updated.terminalTabs[0].panes[0].startupCommand).toBe(`claude --resume ${sessionId}`);
			expect(updated.activeTerminalTabId).toBe(updated.terminalTabs[0].id);
		});

		it('uses new session command when no session ID', () => {
			const tab = makeTab({
				id: 'tab-1',
				type: 'claude',
				panes: [{ id: 'pane-1', type: 'claude' }]
			});
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.restartAISession('ws-a', 'tab-1');

			expect(store.workspaces[0].terminalTabs[0].panes[0].startupCommand).toBe('claude');
		});

		it('no-ops for non-AI tab', () => {
			const tab = makeTab({ id: 'tab-1' }); // no type = shell
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.restartAISession('ws-a', 'tab-1');

			// Tab unchanged
			expect(store.workspaces[0].terminalTabs[0].id).toBe('tab-1');
		});
	});

	describe('updateAITab', () => {
		it('updates the label and session ID', () => {
			const tab = makeTab({
				id: 'tab-1',
				type: 'claude',
				label: 'Claude 1',
				panes: [{ id: 'pane-1', type: 'claude', claudeSessionId: '' }]
			});
			const ws = makeWorkspace({ id: 'ws-a', terminalTabs: [tab] });
			store.workspaces = [ws];

			store.updateAITab('ws-a', 'tab-1', 'session-123', 'Updated Label', 'claude');

			const updated = store.workspaces[0].terminalTabs[0];
			expect(updated.label).toBe('Updated Label');
			expect(updated.panes[0].claudeSessionId).toBe('session-123');
		});
	});

	describe('updateAISessionByPaneId', () => {
		it('updates session ID and startupCommand for valid UUID', () => {
			const tab = makeTab({
				id: 'tab-1',
				type: 'claude',
				panes: [{ id: 'pane-1', type: 'claude', claudeSessionId: '' }]
			});
			const ws = makeWorkspace({ id: 'ws-a', terminalTabs: [tab] });
			store.workspaces = [ws];
			invokeSpy.mockClear();

			const validUuid = 'abcd1234-5678-9012-3456-789012345678';
			store.updateAISessionByPaneId('pane-1', validUuid, 'claude');

			const pane = store.workspaces[0].terminalTabs[0].panes[0];
			expect(pane.claudeSessionId).toBe(validUuid);
			expect(pane.startupCommand).toBe(`claude --resume ${validUuid}`);
			expect(invokeSpy).toHaveBeenCalledWith('save_workspaces', expect.any(Object));
		});

		it('updates session ID without throwing for invalid session ID', () => {
			const tab = makeTab({
				id: 'tab-1',
				type: 'claude',
				panes: [{ id: 'pane-1', type: 'claude', claudeSessionId: '', startupCommand: 'claude' }]
			});
			const ws = makeWorkspace({ id: 'ws-a', terminalTabs: [tab] });
			store.workspaces = [ws];
			invokeSpy.mockClear();

			store.updateAISessionByPaneId('pane-1', 'not-a-uuid', 'claude');

			const pane = store.workspaces[0].terminalTabs[0].panes[0];
			expect(pane.claudeSessionId).toBe('not-a-uuid');
			expect(pane.startupCommand).toBe('claude');
			expect(invokeSpy).toHaveBeenCalledWith('save_workspaces', expect.any(Object));
		});

		it('updates startupCommand for codex session ID', () => {
			const tab = makeTab({
				id: 'tab-1',
				type: 'codex',
				panes: [{ id: 'pane-1', type: 'codex', claudeSessionId: '' }]
			});
			const ws = makeWorkspace({ id: 'ws-a', terminalTabs: [tab] });
			store.workspaces = [ws];
			invokeSpy.mockClear();

			store.updateAISessionByPaneId('pane-1', 'any-session-id', 'codex');

			const pane = store.workspaces[0].terminalTabs[0].panes[0];
			expect(pane.claudeSessionId).toBe('any-session-id');
			expect(pane.startupCommand).toBe('codex resume any-session-id');
		});

		it('does not persist when session ID is already the same', () => {
			const tab = makeTab({
				id: 'tab-1',
				type: 'claude',
				panes: [{ id: 'pane-1', type: 'claude', claudeSessionId: 'same-id' }]
			});
			const ws = makeWorkspace({ id: 'ws-a', terminalTabs: [tab] });
			store.workspaces = [ws];
			invokeSpy.mockClear();

			store.updateAISessionByPaneId('pane-1', 'same-id', 'claude');

			expect(invokeSpy).not.toHaveBeenCalled();
		});
	});

	describe('findAIPaneContext', () => {
		it('finds workspace/tab context for a pane', () => {
			const tab = makeTab({
				id: 'tab-1',
				type: 'claude',
				panes: [{ id: 'pane-1', type: 'claude' }]
			});
			const ws = makeWorkspace({
				id: 'ws-a',
				projectPath: '/projects/test',
				terminalTabs: [tab]
			});
			store.workspaces = [ws];

			const ctx = store.findAIPaneContext('pane-1', 'claude');

			expect(ctx).toEqual({
				workspaceId: 'ws-a',
				tabId: 'tab-1',
				projectPath: '/projects/test'
			});
		});

		it('returns null for unknown pane', () => {
			expect(store.findAIPaneContext('nonexistent')).toBeNull();
		});
	});

	describe('updateAITabLabelByPaneId', () => {
		it('updates tab label for a given pane', () => {
			const tab = makeTab({
				id: 'tab-1',
				type: 'claude',
				label: 'Old Label',
				panes: [{ id: 'pane-1', type: 'claude' }]
			});
			const ws = makeWorkspace({ id: 'ws-a', terminalTabs: [tab] });
			store.workspaces = [ws];

			store.updateAITabLabelByPaneId('pane-1', 'New Label', 'claude');

			expect(store.workspaces[0].terminalTabs[0].label).toBe('New Label');
		});

		it('no-ops when label is already the same', () => {
			const tab = makeTab({
				id: 'tab-1',
				type: 'claude',
				label: 'Same',
				panes: [{ id: 'pane-1', type: 'claude' }]
			});
			const ws = makeWorkspace({ id: 'ws-a', terminalTabs: [tab] });
			store.workspaces = [ws];
			invokeSpy.mockClear();

			store.updateAITabLabelByPaneId('pane-1', 'Same', 'claude');

			// Should not persist since label didn't change
			expect(invokeSpy).not.toHaveBeenCalled();
		});
	});

	// ─── projectPath-based Convenience Methods ──────────────

	describe('selectTabByProject', () => {
		it('selects workspace and tab', () => {
			const tab = makeTab({ id: 'tab-1' });
			const ws = makeWorkspace({
				id: 'ws-a',
				projectPath: '/a',
				terminalTabs: [tab],
				activeTerminalTabId: ''
			});
			store.workspaces = [ws];
			store.selectedId = null;

			store.selectTabByProject('/a', 'tab-1');

			expect(store.selectedId).toBe('ws-a');
			expect(store.workspaces[0].activeTerminalTabId).toBe('tab-1');
		});
	});

	describe('addClaudeByProject', () => {
		it('adds a claude session to the main workspace', () => {
			const ws = makeWorkspace({ id: 'ws-a', projectPath: '/a' });
			store.workspaces = [ws];

			const result = store.addClaudeByProject('/a');

			expect(result).not.toBeNull();
			expect(result!.workspaceId).toBe('ws-a');
			expect(result!.tabId).toBeTruthy();
		});

		it('returns null when no workspace exists for project', () => {
			expect(store.addClaudeByProject('/nonexistent')).toBeNull();
		});
	});

	describe('addAIByProject', () => {
		it('creates a codex session by project path', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a', projectPath: '/a' })];

			const result = store.addAIByProject('/a', 'codex');

			expect(result).not.toBeNull();
			expect(store.workspaces[0].terminalTabs[0].type).toBe('codex');
		});
	});

	describe('runTaskByProject', () => {
		it('adds a task tab to the main workspace', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a', projectPath: '/a' })];

			const result = store.runTaskByProject('/a', { name: 'Test', command: 'bun test' });

			expect(result).not.toBeNull();
			expect(result!.workspaceId).toBe('ws-a');
			expect(store.workspaces[0].terminalTabs[0].label).toBe('Test');
			expect(store.workspaces[0].terminalTabs[0].panes[0].startupCommand).toBe('bun test');
		});

		it('returns null when no workspace exists', () => {
			expect(store.runTaskByProject('/nonexistent', { name: 'T', command: 'c' })).toBeNull();
		});
	});

	describe('runTaskInWorkspace', () => {
		it('sets selectedId and adds task tab', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			store.selectedId = null;

			const result = store.runTaskInWorkspace('ws-a', { name: 'Build', command: 'make' });

			expect(store.selectedId).toBe('ws-a');
			expect(result.workspaceId).toBe('ws-a');
			expect(result.tabId).toBeTruthy();
			expect(store.workspaces[0].terminalTabs[0].label).toBe('Build');
		});
	});

	// ─── Load ───────────────────────────────────────────────

	describe('load', () => {
		it('loads workspaces from saved snapshot', async () => {
			const ws = makeWorkspace({ id: 'saved-ws', projectPath: '/saved' });
			invokeSpy.mockResolvedValueOnce({
				workspaces: [ws],
				selectedId: 'saved-ws'
			});

			await store.load();

			expect(store.workspaces).toHaveLength(1);
			expect(store.workspaces[0].id).toBe('saved-ws');
			expect(store.selectedId).toBe('saved-ws');
		});

		it('does not overwrite state when snapshot is empty', async () => {
			invokeSpy.mockResolvedValueOnce({ workspaces: [], selectedId: null });

			const existing = makeWorkspace({ id: 'existing' });
			store.workspaces = [existing];

			await store.load();

			// Empty snapshot doesn't overwrite
			expect(store.workspaces).toHaveLength(1);
			expect(store.workspaces[0].id).toBe('existing');
		});

		it('handles load failure gracefully', async () => {
			const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
			invokeSpy.mockRejectedValueOnce(new Error('file not found'));

			await store.load();

			expect(warnSpy).toHaveBeenCalledWith(
				'[WorkspaceStore] No saved workspaces:',
				expect.any(Error)
			);
			expect(store.workspaces).toHaveLength(0);
			warnSpy.mockRestore();
		});
	});

	// ─── ensureShape ────────────────────────────────────────

	describe('ensureShape', () => {
		it('adds panes to tabs with empty panes array', () => {
			const tab = makeTab({ id: 'tab-1', panes: [] });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.ensureShape();

			expect(store.workspaces[0].terminalTabs[0].panes).toHaveLength(1);
			expect(store.workspaces[0].terminalTabs[0].panes[0].id).toBeTruthy();
		});

		it('fixes stale activeTerminalTabId', () => {
			const tab = makeTab({ id: 'tab-1' });
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'nonexistent'
			});
			store.workspaces = [ws];

			store.ensureShape();

			expect(store.workspaces[0].activeTerminalTabId).toBe('tab-1');
		});

		it('sets activeTerminalTabId to empty when no tabs exist', () => {
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [],
				activeTerminalTabId: 'stale'
			});
			store.workspaces = [ws];

			store.ensureShape();

			expect(store.workspaces[0].activeTerminalTabId).toBe('');
		});

		it('updates AI resume commands for panes with session IDs', () => {
			const sessionId = '12345678-1234-1234-1234-123456789abc';
			const tab: TerminalTabState = {
				id: 'tab-1',
				label: 'Claude 1',
				split: 'horizontal',
				type: 'claude',
				panes: [
					{
						id: 'pane-1',
						type: 'claude',
						claudeSessionId: sessionId,
						startupCommand: 'wrong-command'
					}
				]
			};
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.ensureShape();

			expect(store.workspaces[0].terminalTabs[0].panes[0].startupCommand).toBe(
				`claude --resume ${sessionId}`
			);
		});

		it('updates AI new session commands for panes without session IDs', () => {
			const tab: TerminalTabState = {
				id: 'tab-1',
				label: 'Claude 1',
				split: 'horizontal',
				type: 'claude',
				panes: [
					{
						id: 'pane-1',
						type: 'claude',
						startupCommand: 'wrong-command'
					}
				]
			};
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.ensureShape();

			expect(store.workspaces[0].terminalTabs[0].panes[0].startupCommand).toBe('claude');
		});

		it('preserves AI new session commands that include prompt args', () => {
			const tab: TerminalTabState = {
				id: 'tab-1',
				label: 'Claude 1',
				split: 'horizontal',
				type: 'claude',
				panes: [
					{
						id: 'pane-1',
						type: 'claude',
						startupCommand: "claude 'Review this PR for regressions'"
					}
				]
			};
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.ensureShape();

			expect(store.workspaces[0].terminalTabs[0].panes[0].startupCommand).toBe(
				"claude 'Review this PR for regressions'"
			);
		});

		it('fixes codex resume commands', () => {
			const sessionId = '12345678-1234-1234-1234-123456789abc';
			const tab: TerminalTabState = {
				id: 'tab-1',
				label: 'Codex 1',
				split: 'horizontal',
				type: 'codex',
				panes: [
					{
						id: 'pane-1',
						type: 'codex',
						claudeSessionId: sessionId,
						startupCommand: 'wrong'
					}
				]
			};
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];

			store.ensureShape();

			expect(store.workspaces[0].terminalTabs[0].panes[0].startupCommand).toBe(
				`codex resume ${sessionId}`
			);
		});

		it('does not modify state when everything is correct', () => {
			const sessionId = '12345678-1234-1234-1234-123456789abc';
			const tab: TerminalTabState = {
				id: 'tab-1',
				label: 'Claude 1',
				split: 'horizontal',
				type: 'claude',
				panes: [
					{
						id: 'pane-1',
						type: 'claude',
						claudeSessionId: sessionId,
						startupCommand: `claude --resume ${sessionId}`
					}
				]
			};
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [tab],
				activeTerminalTabId: 'tab-1'
			});
			store.workspaces = [ws];
			const originalRef = store.workspaces;

			store.ensureShape();

			// Should be the same reference (no reassignment)
			expect(store.workspaces).toBe(originalRef);
		});
	});

	// ─── resolvedBranch ─────────────────────────────────────

	describe('resolvedBranch', () => {
		it('returns git branch for main workspace', () => {
			const ws = makeWorkspace({ projectPath: '/a' });
			store.workspaces = [ws];
			mockGitStore.branchByProject = { '/a': 'feature-x' };

			expect(store.resolvedBranch(ws)).toBe('feature-x');
		});

		it('returns worktree fixed branch for worktree workspace', () => {
			const ws = makeWorkspace({
				projectPath: '/a',
				worktreePath: '/a-wt',
				branch: 'wt-branch'
			});
			store.workspaces = [ws];
			mockGitStore.branchByProject = { '/a': 'main' };

			expect(store.resolvedBranch(ws)).toBe('wt-branch');
		});

		it('falls back to ws.branch when git branch unavailable for main workspace', () => {
			const ws = makeWorkspace({ projectPath: '/a', branch: 'stale-branch' });
			store.workspaces = [ws];
			mockGitStore.branchByProject = {};

			expect(store.resolvedBranch(ws)).toBe('stale-branch');
		});

		it('returns undefined when no branch info exists for main workspace', () => {
			const ws = makeWorkspace({ projectPath: '/a' });
			store.workspaces = [ws];
			mockGitStore.branchByProject = {};

			expect(store.resolvedBranch(ws)).toBeUndefined();
		});

		it('prefers git branch over stale ws.branch for main workspace', () => {
			const ws = makeWorkspace({ projectPath: '/a', branch: 'old-branch' });
			store.workspaces = [ws];
			mockGitStore.branchByProject = { '/a': 'current-branch' };

			expect(store.resolvedBranch(ws)).toBe('current-branch');
		});
	});

	// ─── Persistence ────────────────────────────────────────

	describe('persistence', () => {
		it('calls save_workspaces on every mutation', () => {
			const project = makeProject();
			store.open(project);
			expect(invokeSpy).toHaveBeenCalledWith('save_workspaces', {
				snapshot: {
					workspaces: store.workspaces,
					selectedId: store.selectedId
				}
			});
		});
	});
});
