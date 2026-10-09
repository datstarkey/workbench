import { invokeSpy, clearInvokeMocks } from '../../test/tauri-mocks';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { CHAT_SERVER_UNREACHABLE, CLAUDE_NOT_IN_CHAT, WorkspaceStore } from './workspaces.svelte';
import { deleteServerTerminal } from '$features/terminal/terminal-connection';
import { adoptableTerminals } from '$features/terminal/server-terminals';
import { listAgents, stopAgent, stopAgentForPane } from '$features/chat/agent-api';
import { acquireChat, chatHasHistory, reopenChat } from '$features/chat/chat-registry';
import type {
	AgentSummary,
	ProjectConfig,
	ProjectWorkspace,
	TerminalPaneState,
	TerminalTabState
} from '$types/workbench';

// Mock uid to produce predictable IDs
let uidCounter = 0;
vi.mock('$lib/utils/uid', () => ({
	uid: () => `uid-${++uidCounter}`
}));

vi.mock('$features/terminal/terminal-connection', async (importOriginal) => ({
	...(await importOriginal<typeof import('$features/terminal/terminal-connection')>()),
	deleteServerTerminal: vi.fn()
}));

vi.mock('$features/chat/agent-api', async (importOriginal) => ({
	...(await importOriginal<typeof import('$features/chat/agent-api')>()),
	stopAgent: vi.fn(async () => {}),
	stopAgentForPane: vi.fn(async () => {}),
	listAgents: vi.fn(async (): Promise<AgentSummary[] | null> => null)
}));

vi.mock('$features/chat/chat-registry', async (importOriginal) => ({
	...(await importOriginal<typeof import('$features/chat/chat-registry')>()),
	chatHasHistory: vi.fn(() => true),
	acquireChat: vi.fn(() => ({ chat: { onEnded: null }, created: true })),
	reopenChat: vi.fn()
}));

// Mock context so getGitStore() and getWorkbenchSettingsStore() work outside a component
const mockGitStore = {
	branchByProject: {} as Record<string, string>,
	worktreesByProject: {} as Record<string, { path: string; branch: string }[]>,
	statusByProject: {} as Record<string, { branch: string }>
};
const mockWorkbenchSettingsStore = {
	defaultClaudeView: 'terminal' as 'terminal' | 'chat',
	activeClaudeAccountId: undefined as string | undefined,
	claudeAccounts: [
		{ id: 'work', name: 'Work', configDir: '/w' },
		{ id: 'personal', name: 'Personal', configDir: '/p' }
	],
	launchOptions: {}
};
vi.mock('./context', () => ({
	getGitStore: () => mockGitStore,
	getWorkbenchSettingsStore: () => mockWorkbenchSettingsStore
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
		mockGitStore.statusByProject = {};
		mockWorkbenchSettingsStore.defaultClaudeView = 'terminal';
		mockWorkbenchSettingsStore.activeClaudeAccountId = undefined;
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

	describe('isProjectOpen', () => {
		it('returns true for open projects', () => {
			store.workspaces = [
				makeWorkspace({ projectPath: '/a' }),
				makeWorkspace({ projectPath: '/b' })
			];
			expect(store.isProjectOpen('/a')).toBe(true);
			expect(store.isProjectOpen('/b')).toBe(true);
			expect(store.isProjectOpen('/c')).toBe(false);
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

	describe('closeEndedChat', () => {
		it('closes the tab of a single-pane chat', () => {
			const tab1 = makeTab({ id: 'tab-1', panes: [{ id: 'chat' }] });
			const tab2 = makeTab({ id: 'tab-2' });
			store.workspaces = [makeWorkspace({ id: 'ws-a', terminalTabs: [tab1, tab2] })];

			store.closeEndedChat('chat');

			expect(store.workspaces[0].terminalTabs.map((t) => t.id)).toEqual(['tab-2']);
		});

		it('removes only the pane from a split', () => {
			const tab = makeTab({ id: 'tab-1', panes: [{ id: 'chat' }, { id: 'shell' }] });
			store.workspaces = [makeWorkspace({ id: 'ws-a', terminalTabs: [tab] })];

			store.closeEndedChat('chat');

			expect(store.workspaces[0].terminalTabs[0].panes.map((p) => p.id)).toEqual(['shell']);
		});

		it('ignores a pane that is already gone', () => {
			const tab = makeTab({ id: 'tab-1' });
			store.workspaces = [makeWorkspace({ id: 'ws-a', terminalTabs: [tab] })];

			store.closeEndedChat('missing');

			expect(store.workspaces[0].terminalTabs).toHaveLength(1);
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
		function twoTabs(active = 'tab-1') {
			store.workspaces = [
				makeWorkspace({
					id: 'ws-a',
					terminalTabs: [makeTab({ id: 'tab-1' }), makeTab({ id: 'tab-2' })],
					activeTerminalTabId: active
				})
			];
		}

		it('pairs the active tab with the next one', () => {
			twoTabs();

			store.splitTerminal('ws-a', 'vertical', makeProject());

			expect(store.workspaces[0].splitView).toEqual({
				direction: 'vertical',
				tabIds: ['tab-1', 'tab-2']
			});
			expect(store.workspaces[0].terminalTabs).toHaveLength(2);
		});

		it('pairs the last tab with the previous one', () => {
			twoTabs('tab-2');

			store.splitTerminal('ws-a', 'horizontal', makeProject());

			expect(store.workspaces[0].splitView?.tabIds).toEqual(['tab-2', 'tab-1']);
		});

		it('opens a new shell tab to pair with when there is only one tab', () => {
			store.workspaces = [
				makeWorkspace({
					id: 'ws-a',
					terminalTabs: [makeTab({ id: 'tab-1' })],
					activeTerminalTabId: 'tab-1'
				})
			];

			store.splitTerminal('ws-a', 'horizontal', makeProject());

			const ws = store.workspaces[0];
			expect(ws.terminalTabs).toHaveLength(2);
			expect(ws.splitView?.tabIds).toEqual(['tab-1', ws.terminalTabs[1].id]);
			expect(ws.activeTerminalTabId).toBe('tab-1');
		});

		it('switches direction, then unsplits on the same direction', () => {
			twoTabs();
			store.splitTerminal('ws-a', 'horizontal', makeProject());

			store.splitTerminal('ws-a', 'vertical', makeProject());
			expect(store.workspaces[0].splitView?.direction).toBe('vertical');

			store.splitTerminal('ws-a', 'vertical', makeProject());
			expect(store.workspaces[0].splitView).toBeUndefined();
		});

		it('no-ops when no active tab found', () => {
			const ws = makeWorkspace({
				id: 'ws-a',
				terminalTabs: [],
				activeTerminalTabId: 'nonexistent'
			});
			store.workspaces = [ws];

			store.splitTerminal('ws-a', 'horizontal', makeProject());

			expect(store.workspaces[0].terminalTabs).toHaveLength(0);
			expect(store.workspaces[0].splitView).toBeUndefined();
		});

		it('replaces a split whose tab no longer exists instead of toggling it off', () => {
			twoTabs();
			store.workspaces[0].splitView = { direction: 'horizontal', tabIds: ['tab-1', 'gone'] };

			store.splitTerminal('ws-a', 'horizontal', makeProject());

			expect(store.workspaces[0].splitView?.tabIds).toEqual(['tab-1', 'tab-2']);
		});

		it('no-ops for native workspaces', () => {
			twoTabs();
			store.workspaces[0].renderer = 'native';

			store.splitTerminal('ws-a', 'horizontal', makeProject());

			expect(store.workspaces[0].splitView).toBeUndefined();
		});

		it('keeps the split when a paired AI tab restarts', () => {
			store.workspaces = [
				makeWorkspace({
					id: 'ws-a',
					terminalTabs: [
						makeTab({ id: 'tab-1' }),
						makeTab({ id: 'tab-2', type: 'claude', panes: [{ id: 'p2', type: 'claude' }] })
					],
					activeTerminalTabId: 'tab-1',
					splitView: { direction: 'horizontal', tabIds: ['tab-1', 'tab-2'] }
				})
			];

			store.restartAISession('ws-a', 'tab-2');

			const ws = store.workspaces[0];
			expect(ws.splitView?.tabIds).toEqual(['tab-1', ws.terminalTabs[1].id]);
			expect(ws.terminalTabs[1].id).not.toBe('tab-2');
		});

		it('closing a split tab drops the split', () => {
			twoTabs();
			store.splitTerminal('ws-a', 'horizontal', makeProject());

			store.closeTerminalTab('ws-a', 'tab-2');

			expect(store.workspaces[0].splitView).toBeUndefined();
		});
	});

	describe('reorderTerminalTab', () => {
		it('moves a tab to the target position', () => {
			store.workspaces = [
				makeWorkspace({
					id: 'ws-a',
					terminalTabs: [makeTab({ id: 't1' }), makeTab({ id: 't2' }), makeTab({ id: 't3' })]
				})
			];

			store.reorderTerminalTab('ws-a', 't3', 't1');

			expect(store.workspaces[0].terminalTabs.map((t) => t.id)).toEqual(['t3', 't1', 't2']);
		});

		it('no-ops for unknown ids', () => {
			const tabs = [makeTab({ id: 't1' }), makeTab({ id: 't2' })];
			store.workspaces = [makeWorkspace({ id: 'ws-a', terminalTabs: tabs })];

			store.reorderTerminalTab('ws-a', 't1', 'nope');

			expect(store.workspaces[0].terminalTabs.map((t) => t.id)).toEqual(['t1', 't2']);
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

		it('removes a pane from a split tab that is not the active one', () => {
			store.workspaces = [
				makeWorkspace({
					id: 'ws-a',
					terminalTabs: [
						makeTab({ id: 'tab-1' }),
						makeTab({ id: 'tab-2', panes: [{ id: 'pane-1' }, { id: 'pane-2' }] })
					],
					activeTerminalTabId: 'tab-1'
				})
			];

			store.removePane('ws-a', 'pane-1');

			expect(store.workspaces[0].terminalTabs[1].panes.map((p) => p.id)).toEqual(['pane-2']);
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
			expect(tab.panes[0].startupCommand).toBeUndefined();
			expect(tab.panes[0].claudeSessionId).toMatch(/^[0-9a-f-]{36}$/);
			expect(updated.activeTerminalTabId).toBe(tabId);
		});

		it('opens a new Claude tab as chat when that is the default', () => {
			mockWorkbenchSettingsStore.defaultClaudeView = 'chat';
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];

			store.addAISession('ws-a', 'claude');

			const pane = store.workspaces[0].terminalTabs[0].panes[0];
			expect(pane.view).toBe('chat');
			expect(pane.claudeSessionId).toMatch(/^[0-9a-f-]{36}$/);
		});

		it("opens an agent action's tab as a terminal even when chat is the default", () => {
			mockWorkbenchSettingsStore.defaultClaudeView = 'chat';
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];

			store.addAISession('ws-a', 'claude', { prompt: 'fix the build' });

			expect(store.workspaces[0].terminalTabs[0].panes[0].view).toBeUndefined();
		});

		it('creates a codex tab with correct type', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];

			store.addAISession('ws-a', 'codex');

			const tab = store.workspaces[0].terminalTabs[0];
			expect(tab.type).toBe('codex');
			expect(tab.label).toBe('Codex 1');
			expect(tab.panes[0].type).toBe('codex');
			expect(tab.panes[0].startupCommand).toBe('codex -c tui.alternate_screen=never');
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

		it('accepts explicit label and prompt options', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];

			store.addAISession('ws-a', 'claude', {
				label: 'Review PR',
				prompt: 'Review this PR for regressions'
			});

			const tab = store.workspaces[0].terminalTabs[0];
			expect(tab.label).toBe('Review PR');
			expect(tab.panes[0].claudePrompt).toBe('Review this PR for regressions');
		});
	});

	describe('claude accounts', () => {
		const sessionId = '12345678-1234-1234-1234-123456789abc';

		it('a new claude pane carries no account: the host picks it at launch', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			mockWorkbenchSettingsStore.activeClaudeAccountId = 'work';
			store.addAISession('ws-a', 'claude');
			store.addAISession('ws-a', 'codex');

			const accounts = store.workspaces[0].terminalTabs.map((t) => t.panes[0].claudeAccountId);
			expect(accounts).toEqual([undefined, undefined]);
		});

		it('a pane follows its session onto another account', () => {
			const tab = makeTab({
				id: 'tab-1',
				type: 'claude',
				panes: [{ id: 'pane-1', type: 'claude', claudeSessionId: sessionId }]
			});
			store.workspaces = [makeWorkspace({ id: 'ws-a', terminalTabs: [tab] })];

			store.setPaneClaudeAccount('pane-1', 'work');
			expect(store.workspaces[0].terminalTabs[0].panes[0].claudeAccountId).toBe('work');
			store.setPaneClaudeAccount('pane-1', undefined);
			expect(store.workspaces[0].terminalTabs[0].panes[0].claudeAccountId).toBe(''); // the default login, now known
		});

		it('resume uses the account owning the transcript, not the active one', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			mockWorkbenchSettingsStore.activeClaudeAccountId = 'personal';

			store.resumeAISession('ws-a', sessionId, 'Old', 'claude', 'work');

			expect(store.workspaces[0].terminalTabs[0].panes[0].claudeAccountId).toBe('work');
		});

		it('restart keeps the pane on its account', () => {
			const tab = makeTab({
				id: 'tab-1',
				type: 'claude',
				panes: [
					{ id: 'pane-1', type: 'claude', claudeSessionId: sessionId, claudeAccountId: 'work' }
				]
			});
			store.workspaces = [
				makeWorkspace({ id: 'ws-a', terminalTabs: [tab], activeTerminalTabId: 'tab-1' })
			];
			mockWorkbenchSettingsStore.activeClaudeAccountId = undefined;

			store.restartAISession('ws-a', 'tab-1');

			expect(store.workspaces[0].terminalTabs[0].panes[0].claudeAccountId).toBe('work');
		});

		it('a login task tab runs its shell under the given account', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];

			store.runTaskInWorkspace(
				'ws-a',
				{ name: 'Claude login', command: 'claude auth login' },
				'work'
			);

			const pane = store.workspaces[0].terminalTabs[0].panes[0];
			expect(pane).toMatchObject({ startupCommand: 'claude auth login', claudeAccountId: 'work' });
		});
	});

	describe('chat view guards', () => {
		const id = '32345678-1234-1234-1234-123456789abc';

		function summary(overrides: Partial<AgentSummary>): AgentSummary {
			return {
				agent: 'claude',
				sessionId: id,
				projectPath: '/projects/test',
				worktreePath: null,
				paneId: null,
				claudeAccountId: null,
				title: null,
				model: null,
				busy: false,
				exited: false,
				busySince: null,
				updatedAt: 1,
				waiting: null,
				running: null,
				previousIds: [],
				...overrides
			};
		}

		it('a resume focuses the tab running the session only while it is live there', async () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' }), makeWorkspace({ id: 'ws-b' })];
			store.resumeAISession('ws-a', id, 'S', 'claude', 'work');
			const tabId = store.workspaces[0].terminalTabs[0].id;
			store.resumeAISession('ws-a', '42345678-1234-1234-1234-123456789abc', 'Other', 'claude');
			store.selectedId = 'ws-b';

			vi.mocked(listAgents).mockResolvedValueOnce([summary({ exited: true })]);
			expect(await store.focusLiveSession(id, 'work')).toBe(false);
			expect(await store.focusLiveSession(id, undefined)).toBe(false);
			expect(store.selectedId).toBe('ws-b');

			vi.mocked(listAgents).mockResolvedValueOnce([summary({})]);
			expect(await store.focusLiveSession(id, 'work')).toBe(true);
			expect(store.selectedId).toBe('ws-a');
			expect(store.workspaces[0].activeTerminalTabId).toBe(tabId);
		});

		it("Chat attaches to the terminal's own claude, found by pane or terminal", async () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			store.resumeAISession('ws-a', id, 'S', 'claude');
			const paneId = store.workspaces[0].terminalTabs[0].panes[0].id;
			store.setServerTerminalId(paneId, 'srv-1');
			vi.mocked(listAgents).mockResolvedValueOnce([summary({ terminalId: 'srv-1' })]);

			await store.setPaneView(paneId, 'chat');

			expect(store.workspaces[0].terminalTabs[0].panes[0]).toMatchObject({
				view: 'chat',
				liveTerminal: true,
				claudeSessionId: id
			});
			expect(deleteServerTerminal).not.toHaveBeenCalled();
		});

		it("Chat refuses when Claude hasn't connected, and Terminal only switches the view", async () => {
			vi.mocked(stopAgent).mockClear();
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			store.resumeAISession('ws-a', id, 'S', 'claude');
			const paneId = store.workspaces[0].terminalTabs[0].panes[0].id;
			vi.mocked(listAgents).mockResolvedValueOnce(null);
			await expect(store.setPaneView(paneId, 'chat')).rejects.toThrow(CHAT_SERVER_UNREACHABLE);
			vi.mocked(listAgents).mockResolvedValueOnce([summary({ paneId, exited: true })]);
			await expect(store.setPaneView(paneId, 'chat')).rejects.toThrow(CLAUDE_NOT_IN_CHAT);
			expect(store.isChatPane(paneId)).toBe(false);

			vi.mocked(listAgents).mockResolvedValueOnce([summary({ paneId })]);
			await store.setPaneView(paneId, 'chat');
			await store.setPaneView(paneId, 'terminal');
			expect(store.workspaces[0].terminalTabs[0].panes[0]).toMatchObject({
				view: 'terminal',
				liveTerminal: undefined,
				claudeSessionId: id
			});
			expect(stopAgent).not.toHaveBeenCalled();
		});

		it('keeps a restarted chat tab in chat', async () => {
			mockWorkbenchSettingsStore.defaultClaudeView = 'chat';
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			store.resumeAISession('ws-a', id, 'S', 'claude');
			const tabId = store.workspaces[0].terminalTabs[0].id;

			await store.restartAISession('ws-a', tabId);
			const restarted = store.workspaces[0].terminalTabs[0];
			expect(restarted.panes[0].view).toBe('chat');
			expect(restarted.panes[0].claudeSessionId).toBe(id);
		});
	});

	describe('chat resume', () => {
		const older = '12345678-1234-1234-1234-123456789abc';
		const current = '22345678-1234-1234-1234-123456789abc';

		it('opens a resumed session as chat when that is the default', () => {
			mockWorkbenchSettingsStore.defaultClaudeView = 'chat';
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			store.resumeAISession('ws-a', older, 'Older', 'claude');
			const pane = store.workspaces[0].terminalTabs[0].panes[0];
			expect(pane.view).toBe('chat');
			expect(pane.claudeSessionId).toBe(older);
		});

		it('swaps a chat pane to another conversation in place', async () => {
			mockWorkbenchSettingsStore.defaultClaudeView = 'chat';
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			store.resumeAISession('ws-a', current, 'Current', 'claude');
			const paneId = store.workspaces[0].terminalTabs[0].panes[0].id;

			await store.resumeInChat(paneId, older, 'Older');

			const tab = store.workspaces[0].terminalTabs[0];
			expect(tab.panes[0].claudeSessionId).toBe(older);
			expect(tab.panes[0].view).toBe('chat');
			expect(tab.label).toBe('Older');
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
			expect(tab.panes[0].startupCommand).toBeUndefined();
			expect(store.workspaces[0].activeTerminalTabId).toBe(tab.id);
		});

		it('creates a tab with resume command for codex', () => {
			const ws = makeWorkspace({ id: 'ws-a' });
			store.workspaces = [ws];
			const sessionId = '12345678-1234-1234-1234-123456789abc';

			store.resumeAISession('ws-a', sessionId, 'My Codex', 'codex');

			const tab = store.workspaces[0].terminalTabs[0];
			expect(tab.type).toBe('codex');
			expect(tab.panes[0].startupCommand).toBe(
				`codex -c tui.alternate_screen=never resume ${sessionId}`
			);
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
			expect(updated.terminalTabs[0].panes[0].startupCommand).toBeUndefined();
			expect(updated.activeTerminalTabId).toBe(updated.terminalTabs[0].id);
			// A restart isn't an End: other devices keep the chat.
			expect(stopAgentForPane).toHaveBeenLastCalledWith('pane-1', { end: false });
		});

		it('starts a new session when the pane has no session ID', () => {
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

			const pane = store.workspaces[0].terminalTabs[0].panes[0];
			expect(pane.claudeSessionId).toMatch(/^[0-9a-f-]{36}$/);
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

	describe('updateAISessionByPaneId', () => {
		it('updates the session ID of a Claude pane, which has no command', () => {
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
			expect(pane.startupCommand).toBeUndefined();
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

			store.updateAISessionByPaneId('pane-1', '01a0f8bd-343d-7ae3-a22b-74d35455ef7f', 'codex');

			const pane = store.workspaces[0].terminalTabs[0].panes[0];
			expect(pane.claudeSessionId).toBe('01a0f8bd-343d-7ae3-a22b-74d35455ef7f');
			expect(pane.startupCommand).toBe(
				'codex -c tui.alternate_screen=never resume 01a0f8bd-343d-7ae3-a22b-74d35455ef7f'
			);
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
				projectPath: '/projects/test',
				cwd: '/projects/test'
			});
		});

		it('reports the worktree path as cwd for a worktree workspace', () => {
			// Session JSONL is keyed by the directory the pane actually runs in, so a
			// worktree tab must not resolve to its parent project path.
			const tab = makeTab({
				id: 'tab-1',
				type: 'claude',
				panes: [{ id: 'pane-1', type: 'claude' }]
			});
			const ws = makeWorkspace({
				id: 'ws-a',
				projectPath: '/projects/test',
				worktreePath: '/projects/test-feature',
				terminalTabs: [tab]
			});
			store.workspaces = [ws];

			const ctx = store.findAIPaneContext('pane-1', 'claude');

			expect(ctx?.cwd).toBe('/projects/test-feature');
			expect(ctx?.projectPath).toBe('/projects/test');
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

	describe('addAIByProject', () => {
		it('creates a codex session by project path', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a', projectPath: '/a' })];

			const result = store.addAIByProject('/a', 'codex');

			expect(result).not.toBeNull();
			expect(store.workspaces[0].terminalTabs[0].type).toBe('codex');
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
				`codex -c tui.alternate_screen=never resume ${sessionId}`
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
						claudeSessionId: sessionId
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

	// ─── activeGitStatus ────────────────────────────────────

	describe('activeGitStatus', () => {
		it('is undefined without an active workspace', () => {
			expect(store.activeGitStatus).toBeUndefined();
		});

		it('reads the worktree path for worktree workspaces', () => {
			const ws = makeWorkspace({ projectPath: '/a', worktreePath: '/a-wt' });
			mockGitStore.statusByProject = { '/a': { branch: 'main' }, '/a-wt': { branch: 'wt' } };
			store.workspaces = [ws];

			expect(store.activeGitStatus).toEqual({ branch: 'wt' });
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
					selectedId: store.selectedId,
					serverTerminalIds: {}
				}
			});
		});
	});

	// ─── Claude panes run a session, never a command ────────

	describe('Claude pane launches', () => {
		const sessionId = '12345678-1234-1234-1234-123456789abc';

		function pane(): TerminalPaneState {
			return store.workspaces[0].terminalTabs[0].panes[0];
		}

		function seedPane(seed: TerminalPaneState) {
			store.workspaces = [
				makeWorkspace({
					id: 'ws-a',
					terminalTabs: [
						{ id: 'tab-1', label: 'AI 1', split: 'horizontal', type: seed.type, panes: [seed] }
					],
					activeTerminalTabId: 'tab-1'
				})
			];
		}

		it('a new Claude tab picks its session id up front and builds no command', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];

			store.addAISession('ws-a', 'claude');

			expect(pane().claudeSessionId).toMatch(/^[0-9a-f-]{36}$/);
			expect(pane().startupCommand).toBeUndefined();
		});

		it("an agent action's prompt rides along for the server to quote", () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];

			store.addAISession('ws-a', 'claude', { prompt: "  it's broken  " });

			expect(pane().claudePrompt).toBe("it's broken");
			expect(pane().view).toBeUndefined();
		});

		it("a Codex agent action's prompt is in its command", () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];

			store.addAISession('ws-a', 'codex', { prompt: 'audit' });

			expect(pane().startupCommand).toBe("codex -c tui.alternate_screen=never 'audit'");
		});

		it('a restart keeps the session and its prompt', async () => {
			seedPane({
				id: 'pane-1',
				type: 'claude',
				claudeSessionId: sessionId,
				claudePrompt: 'Review'
			});

			await store.restartAISession('ws-a', 'tab-1');

			expect(pane()).toMatchObject({ claudeSessionId: sessionId, claudePrompt: 'Review' });
		});

		it('drops a command an older build saved', () => {
			seedPane({
				id: 'pane-1',
				type: 'claude',
				claudeSessionId: sessionId,
				startupCommand: `npx --yes @anthropic-ai/sandbox-runtime@0.0.76 --settings '/x' -- claude --resume ${sessionId}`
			});

			store.ensureShape();

			expect(pane()).toEqual({ id: 'pane-1', type: 'claude', claudeSessionId: sessionId });
		});

		it("keeps a Codex pane's prompt while its command follows the launch options", () => {
			seedPane({ id: 'pane-1', type: 'codex', startupCommand: "codex 'Find DRY violations'" });

			store.ensureShape();

			expect(pane().startupCommand).toBe(
				"codex -c tui.alternate_screen=never 'Find DRY violations'"
			);
		});

		it('gives an id-less Claude pane from an older build a new session', () => {
			seedPane({ id: 'pane-1', type: 'claude', claudeSessionId: '', startupCommand: 'claude' });

			store.ensureShape();

			expect(pane().claudeSessionId).toMatch(/^[0-9a-f-]{36}$/);
			expect(pane().startupCommand).toBeUndefined();
		});
	});
	// ─── Adopting terminals opened on another device ────────

	describe('adoptServerTerminal', () => {
		const remote = { id: 'srv-remote', name: 'phone shell', cwd: '/projects/test', alive: true };

		it('adds a background tab mapped to the existing server terminal, starting detached', () => {
			const active = makeTab({ id: 'tab-active' });
			const ws = makeWorkspace({ terminalTabs: [active], activeTerminalTabId: 'tab-active' });
			store.workspaces = [ws];

			expect(store.adoptServerTerminal(remote)).toBe(true);

			const tabs = store.workspaces[0].terminalTabs;
			expect(tabs).toHaveLength(2);
			const adopted = tabs[1];
			expect(adopted.label).toBe('phone shell');
			expect(adopted.panes[0].startupCommand).toBeUndefined();
			const paneId = adopted.panes[0].id;
			expect(store.getServerTerminalId(paneId)).toBe('srv-remote');
			expect(store.knownServerTerminalIds()).toContain('srv-remote');
			expect(store.isAdoptedPane(paneId)).toBe(true);
			// Doesn't steal focus from the tab the user is on.
			expect(store.workspaces[0].activeTerminalTabId).toBe('tab-active');
			expect(invokeSpy).toHaveBeenCalledWith('save_workspaces', expect.anything());
		});

		it('routes a worktree cwd to the worktree workspace', () => {
			store.workspaces = [
				makeWorkspace({ id: 'main' }),
				makeWorkspace({ id: 'wt', worktreePath: '/projects/test-feat', branch: 'feat' })
			];

			store.adoptServerTerminal({ ...remote, cwd: '/projects/test-feat' });

			expect(store.workspaces.find((w) => w.id === 'wt')!.terminalTabs).toHaveLength(1);
			expect(store.workspaces.find((w) => w.id === 'main')!.terminalTabs).toHaveLength(0);
		});

		it('does nothing when no open workspace runs in the cwd', () => {
			store.workspaces = [makeWorkspace()];

			expect(store.adoptServerTerminal({ ...remote, cwd: '/elsewhere' })).toBe(false);
			expect(store.workspaces[0].terminalTabs).toHaveLength(0);
			expect(store.knownServerTerminalIds()).toEqual([]);
		});

		it('falls back to a generic label for an unnamed terminal', () => {
			store.workspaces = [makeWorkspace()];
			store.adoptServerTerminal({ ...remote, name: undefined });
			expect(store.workspaces[0].terminalTabs[0].label).toBe('Remote terminal');
		});

		/** The workspaces array from the latest save_workspaces call. */
		const lastSnapshot = () =>
			invokeSpy.mock.calls.filter((c) => c[0] === 'save_workspaces').slice(-1)[0]?.[1] as {
				snapshot: { workspaces: ProjectWorkspace[]; serverTerminalIds: Record<string, string> };
			};

		it('closing an adopted tab kills its terminal and does not re-adopt it on the next poll', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			store.adoptServerTerminal(remote);
			const tab = store.workspaces[0].terminalTabs[0];
			vi.mocked(deleteServerTerminal).mockClear();

			store.closeTerminalTab('ws-a', tab.id);

			expect(deleteServerTerminal).toHaveBeenCalledWith(remote.id);
			expect(store.workspaces[0].terminalTabs).toHaveLength(0);
			const known = new Set(store.knownServerTerminalIds());
			expect(adoptableTerminals([remote], known, () => false)).toEqual([]);
		});

		it('closing a normal tab kills its server terminal without stopping an agent', () => {
			const tab = makeTab({ id: 'tab-own' });
			store.workspaces = [makeWorkspace({ id: 'ws-a', terminalTabs: [tab] })];
			store.setServerTerminalId(tab.panes[0].id, 'srv-own');
			vi.mocked(deleteServerTerminal).mockClear();
			vi.mocked(stopAgentForPane).mockClear();

			store.closeTerminalTab('ws-a', 'tab-own');

			expect(deleteServerTerminal).toHaveBeenCalledWith('srv-own');
			expect(stopAgentForPane).not.toHaveBeenCalled();
		});

		it('leaves adopted tabs and their server ids out of the persisted snapshot', () => {
			const own = makeTab({ id: 'tab-own' });
			store.workspaces = [
				makeWorkspace({ id: 'ws-a', terminalTabs: [own], activeTerminalTabId: 'tab-own' })
			];
			store.setServerTerminalId(own.panes[0].id, 'srv-own');
			store.adoptServerTerminal(remote);
			const adoptedTab = store.workspaces[0].terminalTabs[1];
			store.setActiveTab('ws-a', adoptedTab.id);

			const { snapshot } = lastSnapshot();
			expect(snapshot.workspaces[0].terminalTabs.map((t) => t.id)).toEqual(['tab-own']);
			expect(snapshot.workspaces[0].activeTerminalTabId).toBe('tab-own');
			expect(snapshot.serverTerminalIds).toEqual({ [own.panes[0].id]: 'srv-own' });
			// The live state still shows the adopted tab.
			expect(store.workspaces[0].terminalTabs).toHaveLength(2);
		});

		it('gives panes a readable server-side name', () => {
			store.workspaces = [
				makeWorkspace({ terminalTabs: [makeTab({ id: 't', label: 'Claude 1' })] })
			];
			const paneId = store.workspaces[0].terminalTabs[0].panes[0].id;
			expect(store.paneDisplayName(paneId)).toBe('Test Project · Claude 1');
		});
	});

	// ─── Adopting chats started on another device ───────────

	describe('adoptServerChat', () => {
		const remote: AgentSummary = {
			agent: 'claude',
			sessionId: 'sess-phone',
			projectPath: '/projects/test',
			worktreePath: null,
			paneId: null,
			claudeAccountId: 'work',
			title: 'Fix the login bug',
			model: null,
			busy: true,
			exited: false,
			busySince: null,
			updatedAt: 0,
			waiting: null,
			running: null,
			previousIds: []
		};
		const project = { name: 'test', path: '/projects/test' };
		const adopt = (chat: AgentSummary = remote) =>
			store.adoptServerChat(chat, chat.projectPath === project.path ? project : undefined);
		const adoptable = (list = [remote]) => store.adoptableServerChats(list);
		const lastSnapshot = () =>
			invokeSpy.mock.calls.filter((c) => c[0] === 'save_workspaces').slice(-1)[0]?.[1] as {
				snapshot: { workspaces: ProjectWorkspace[] };
			};

		beforeEach(() => {
			vi.mocked(stopAgent).mockClear();
			vi.mocked(stopAgentForPane).mockClear();
			vi.mocked(deleteServerTerminal).mockClear();
			vi.mocked(reopenChat).mockClear();
		});

		it('adds a background chat tab on the running session', () => {
			const active = makeTab({ id: 'tab-active' });
			store.workspaces = [
				makeWorkspace({ terminalTabs: [active], activeTerminalTabId: 'tab-active' })
			];

			expect(adopt()).toBe(true);

			const tab = store.workspaces[0].terminalTabs[1];
			expect(tab).toMatchObject({ label: 'Fix the login bug', type: 'claude' });
			expect(tab.panes[0]).toMatchObject({
				type: 'claude',
				claudeSessionId: 'sess-phone',
				view: 'chat',
				claudeAccountId: 'work'
			});
			expect(tab.panes[0].startupCommand).toBeUndefined();
			expect(store.isAdoptedPane(tab.panes[0].id)).toBe(true);
			expect(store.workspaces[0].activeTerminalTabId).toBe('tab-active');
			expect(adoptable()).toEqual([]);
		});

		it('follows a /clear re-key instead of adopting the new id as a second tab', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			adopt();
			const rekeyed = { ...remote, sessionId: 'sess-cleared', previousIds: ['sess-phone'] };

			expect(adoptable([rekeyed])).toEqual([]);

			const tabs = store.workspaces[0].terminalTabs;
			expect(tabs).toHaveLength(1);
			expect(tabs[0].panes[0].claudeSessionId).toBe('sess-cleared');
		});

		it('opens a background workspace when none hosts the chat, unsaved until it holds its own tab', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a', projectPath: '/projects/other' })];
			store.selectedId = 'ws-a';

			expect(adopt()).toBe(true);

			const host = store.workspaces.find((w) => w.projectPath === '/projects/test')!;
			expect(host).toMatchObject({ renderer: 'xterm', projectName: 'test' });
			expect(host.terminalTabs[0].panes[0].claudeSessionId).toBe('sess-phone');
			expect(store.selectedId).toBe('ws-a');
			expect(lastSnapshot().snapshot.workspaces.map((w) => w.id)).toEqual(['ws-a']);

			store.closeTerminalTab(host.id, host.terminalTabs[0].id);
			expect(store.workspaces.map((w) => w.id)).toEqual(['ws-a']);
		});

		it('opens a worktree chat its own worktree workspace', () => {
			store.workspaces = [];
			mockGitStore.worktreesByProject = {
				'/projects/test': [{ path: '/projects/test-feat', branch: 'feat' }]
			};
			adopt({ ...remote, worktreePath: '/projects/test-feat' });
			expect(store.workspaces[0]).toMatchObject({
				worktreePath: '/projects/test-feat',
				branch: 'feat'
			});
			mockGitStore.worktreesByProject = {};
		});

		it('keeps adopted tab labels in step with the chat title', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			adopt({ ...remote, title: null });
			adoptable([{ ...remote, title: 'Named later' }]);
			expect(store.workspaces[0].terminalTabs[0].label).toBe('Named later');
		});

		it('routes a worktree chat to the worktree workspace and skips unknown cwds', () => {
			store.workspaces = [
				makeWorkspace({ id: 'main' }),
				makeWorkspace({ id: 'wt', worktreePath: '/projects/test-feat', branch: 'feat' })
			];

			adopt({ ...remote, worktreePath: '/projects/test-feat' });
			expect(adopt({ ...remote, projectPath: '/elsewhere' })).toBe(false);

			expect(store.workspaces.find((w) => w.id === 'wt')!.terminalTabs).toHaveLength(1);
			expect(store.workspaces.find((w) => w.id === 'main')!.terminalTabs).toHaveLength(0);
		});

		it('falls back to a generic label for an untitled chat', () => {
			store.workspaces = [makeWorkspace()];
			adopt({ ...remote, title: null });
			expect(store.workspaces[0].terminalTabs[0].label).toBe('Remote chat');
		});

		it('leaves adopted chats out of the persisted snapshot', () => {
			const own = makeTab({ id: 'tab-own' });
			store.workspaces = [
				makeWorkspace({ id: 'ws-a', terminalTabs: [own], activeTerminalTabId: 'tab-own' })
			];
			adopt();
			store.setActiveTab('ws-a', store.workspaces[0].terminalTabs[1].id);

			const { snapshot } = lastSnapshot();
			expect(snapshot.workspaces[0].terminalTabs.map((t) => t.id)).toEqual(['tab-own']);
			expect(store.workspaces[0].terminalTabs).toHaveLength(2);
		});

		it('closing an adopted chat ends it everywhere and does not re-adopt it', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			adopt();
			const tab = store.workspaces[0].terminalTabs[0];

			store.closeTerminalTab('ws-a', tab.id);

			expect(stopAgent).toHaveBeenCalledWith('sess-phone', { end: true });
			expect(stopAgentForPane).not.toHaveBeenCalled();
			expect(store.workspaces[0].terminalTabs).toHaveLength(0);
			expect(adoptable()).toEqual([]);
		});

		it('adopts a closed chat again once it ended and the other device continued it', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			adopt();
			store.closeTerminalTab('ws-a', store.workspaces[0].terminalTabs[0].id);

			expect(adoptable()).toEqual([]);
			expect(adoptable([])).toEqual([]);
			expect(adoptable()).toEqual([remote]);
		});

		it('restarting an adopted chat re-attaches it instead of stopping the process', async () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			adopt();
			const tab = store.workspaces[0].terminalTabs[0];

			await store.restartAISession('ws-a', tab.id);

			expect(reopenChat).toHaveBeenCalledWith(tab.panes[0].id);
			expect(stopAgent).not.toHaveBeenCalled();
			expect(store.workspaces[0].terminalTabs[0]).toBe(tab);
		});

		it('a takeover makes the pane its own: persisted, and stopped on close', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			adopt();
			const tab = store.workspaces[0].terminalTabs[0];
			const paneId = tab.panes[0].id;

			store.takeOverPane(paneId);

			expect(store.isAdoptedPane(paneId)).toBe(false);
			expect(lastSnapshot().snapshot.workspaces[0].terminalTabs).toHaveLength(1);
			store.closeTerminalTab('ws-a', tab.id);
			expect(stopAgentForPane).toHaveBeenCalledWith(paneId, { end: true });
		});

		it('closing an own chat stops it and does not adopt it while it stops', () => {
			const own = makeTab({
				id: 'tab-own',
				type: 'claude',
				panes: [{ id: 'pane-own', type: 'claude', claudeSessionId: 'sess-own', view: 'chat' }]
			});
			store.workspaces = [makeWorkspace({ id: 'ws-a', terminalTabs: [own] })];

			store.closeTerminalTab('ws-a', 'tab-own');

			expect(stopAgentForPane).toHaveBeenCalledWith('pane-own', { end: true });
			expect(adoptable([{ ...remote, sessionId: 'sess-own', paneId: 'pane-own' }])).toEqual([]);
		});

		it('closing an own chat ended elsewhere stops nothing again', () => {
			const own = makeTab({
				id: 'tab-own',
				type: 'claude',
				panes: [{ id: 'pane-own', type: 'claude', claudeSessionId: 'sess-own', view: 'chat' }]
			});
			store.workspaces = [makeWorkspace({ id: 'ws-a', terminalTabs: [own] })];
			store.setServerTerminalId('pane-own', 'term-own');

			store.closeEndedChat('pane-own');

			expect(store.workspaces[0].terminalTabs).toHaveLength(0);
			expect(stopAgentForPane).not.toHaveBeenCalled();
			expect(deleteServerTerminal).not.toHaveBeenCalled();
		});

		it('closing an adopted chat ended elsewhere only detaches it', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			adopt();

			store.closeEndedChat(store.workspaces[0].terminalTabs[0].panes[0].id);

			expect(store.workspaces[0].terminalTabs).toHaveLength(0);
			expect(stopAgentForPane).not.toHaveBeenCalled();
			expect(adoptable([])).toEqual([]);
		});

		it('joins an adopted chat at once, so an End on the other device closes its tab', () => {
			const own = makeTab({ id: 'tab-own' });
			store.workspaces = [makeWorkspace({ id: 'ws-a', terminalTabs: [own] })];
			adopt();
			const paneId = store.workspaces[0].terminalTabs[1].panes[0].id;

			expect(acquireChat).toHaveBeenLastCalledWith(paneId, {
				agent: 'claude',
				projectPath: '/projects/test',
				sessionId: 'sess-phone',
				paneId,
				claudeAccountId: 'work',
				attachOnly: true
			});
			const results = vi.mocked(acquireChat).mock.results;
			const { chat } = results[results.length - 1].value as { chat: { onEnded: () => void } };
			chat.onEnded();

			expect(store.workspaces[0].terminalTabs.map((t) => t.id)).toEqual(['tab-own']);
			expect(stopAgentForPane).not.toHaveBeenCalled();
		});

		it('keeps an adopted chat the server stops listing: it may have exited or be restarting', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			adopt();

			adoptable([]);

			expect(store.workspaces[0].terminalTabs).toHaveLength(1);
		});

		it('closing an own chat ends it and kills its terminal', () => {
			const own = makeTab({
				id: 'tab-own',
				type: 'claude',
				panes: [{ id: 'pane-own', type: 'claude', view: 'chat', liveTerminal: true }]
			});
			store.workspaces = [makeWorkspace({ id: 'ws-a', terminalTabs: [own] })];
			store.setServerTerminalId('pane-own', 'term-own');

			store.closeTerminalTab('ws-a', 'tab-own');

			expect(stopAgentForPane).toHaveBeenCalledWith('pane-own', { end: true });
			expect(deleteServerTerminal).toHaveBeenCalledWith('term-own');
		});

		it('closing a workspace ends a terminal-view Claude pane and kills its terminal', () => {
			const own = makeTab({
				id: 'tab-own',
				type: 'claude',
				panes: [{ id: 'pane-own', type: 'claude', view: 'terminal' }]
			});
			store.workspaces = [makeWorkspace({ id: 'ws-a', terminalTabs: [own] })];
			store.setServerTerminalId('pane-own', 'term-own');

			store.close('ws-a');

			expect(stopAgentForPane).toHaveBeenCalledWith('pane-own', { end: true });
			expect(deleteServerTerminal).toHaveBeenCalledWith('term-own');
		});

		it('switches an adopted Claude chat through its existing terminal and ends it on close', async () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			adopt({ ...remote, terminalId: 'phone-terminal' });
			const tab = store.workspaces[0].terminalTabs[0];
			const paneId = tab.panes[0].id;
			expect(store.getServerTerminalId(paneId)).toBe('phone-terminal');

			await store.setPaneView(paneId, 'terminal');
			expect(store.isChatPane(paneId)).toBe(false);
			await store.setPaneView(paneId, 'chat');
			expect(store.isChatPane(paneId)).toBe(true);
			expect(store.isAdoptedPane(paneId)).toBe(true);
			expect(stopAgent).not.toHaveBeenCalled();
			expect(deleteServerTerminal).not.toHaveBeenCalled();
			expect(lastSnapshot().snapshot.workspaces[0].terminalTabs).toHaveLength(0);
			store.closeTerminalTab('ws-a', tab.id);
			expect(stopAgentForPane).not.toHaveBeenCalled();
			expect(stopAgent).toHaveBeenCalledWith('sess-phone', { end: true });
			expect(deleteServerTerminal).toHaveBeenCalledWith('phone-terminal');
		});

		it('follows a replaced adopted terminal without attaching it underneath chat', () => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			adopt({ ...remote, terminalId: 'old-terminal' });
			const paneId = store.workspaces[0].terminalTabs[0].panes[0].id;
			store.linkLiveTerminal(paneId, 'replacement-terminal');
			expect(store.getServerTerminalId(paneId)).toBe('replacement-terminal');
			expect(store.isLiveTerminalPane(paneId)).toBe(false);
			expect(store.isAdoptedPane(paneId)).toBe(true);
		});
	});

	describe('codex chat', () => {
		const thread = '0199a213-81c0-7800-8aa1-bbab2a035a53';
		const codexPane = () => store.workspaces[0].terminalTabs[0].panes[0];

		beforeEach(() => {
			store.workspaces = [makeWorkspace({ id: 'ws-a' })];
			store.addAISession('ws-a', 'codex');
			vi.mocked(stopAgent).mockClear();
			vi.mocked(stopAgentForPane).mockClear();
			vi.mocked(deleteServerTerminal).mockClear();
			vi.mocked(chatHasHistory).mockReturnValue(true);
		});

		it('moves to chat by killing the PTY', async () => {
			store.setServerTerminalId(codexPane().id, 'srv-1');

			await store.setPaneView(codexPane().id, 'chat');

			expect(deleteServerTerminal).toHaveBeenCalledWith('srv-1');
			expect(store.getServerTerminalId(codexPane().id)).toBeUndefined();
			expect(codexPane().view).toBe('chat');
		});

		it('keeps the thread id a new chat reports, and resumes it in the terminal', async () => {
			await store.setPaneView(codexPane().id, 'chat');
			store.updateAISessionByPaneId(codexPane().id, thread, 'codex');
			expect(codexPane().claudeSessionId).toBe(thread);

			await store.setPaneView(codexPane().id, 'terminal');

			expect(stopAgent).toHaveBeenCalledWith(thread);
			expect(codexPane()).toMatchObject({
				view: 'terminal',
				claudeSessionId: thread,
				startupCommand: `codex -c tui.alternate_screen=never resume ${thread}`
			});
		});

		it('starts a fresh codex in the terminal when the chat never had a message', async () => {
			await store.setPaneView(codexPane().id, 'chat');
			store.updateAISessionByPaneId(codexPane().id, thread, 'codex');
			vi.mocked(chatHasHistory).mockReturnValue(false);

			await store.setPaneView(codexPane().id, 'terminal');

			expect(codexPane()).toMatchObject({
				claudeSessionId: '',
				startupCommand: 'codex -c tui.alternate_screen=never'
			});
		});

		it('stops a chat with no thread id yet by its pane', async () => {
			await store.setPaneView(codexPane().id, 'chat');
			await store.setPaneView(codexPane().id, 'terminal');

			expect(stopAgentForPane).toHaveBeenCalledWith(codexPane().id);
			expect(stopAgent).not.toHaveBeenCalled();
		});

		it('restarts a chat pane in chat, a never-used thread as a new one', async () => {
			await store.setPaneView(codexPane().id, 'chat');
			store.updateAISessionByPaneId(codexPane().id, thread, 'codex');
			vi.mocked(chatHasHistory).mockReturnValue(false);

			await store.restartAISession('ws-a', store.workspaces[0].terminalTabs[0].id);

			expect(stopAgent).toHaveBeenCalledWith(thread);
			expect(codexPane()).toMatchObject({ type: 'codex', view: 'chat', claudeSessionId: '' });
		});

		it('resumes another thread in place and relabels the Codex tab', async () => {
			await store.setPaneView(codexPane().id, 'chat');
			await store.resumeInChat(codexPane().id, thread, 'Older');

			expect(codexPane().claudeSessionId).toBe(thread);
			expect(store.workspaces[0].terminalTabs[0].label).toBe('Older');
		});

		it('adopts a Codex chat from another device as a background Codex tab', () => {
			const summary: AgentSummary = {
				agent: 'codex',
				sessionId: thread,
				projectPath: '/projects/test',
				worktreePath: null,
				paneId: null,
				claudeAccountId: 'work',
				title: 'Phone thread',
				model: null,
				busy: false,
				exited: false,
				busySince: null,
				updatedAt: 0,
				waiting: null,
				running: null,
				previousIds: []
			};

			expect(store.adoptableServerChats([summary])).toEqual([summary]);
			expect(store.adoptServerChat(summary, undefined)).toBe(true);

			const tab = store.workspaces[0].terminalTabs[1];
			expect(tab.type).toBe('codex');
			expect(tab.panes[0]).toEqual({
				id: tab.panes[0].id,
				type: 'codex',
				claudeSessionId: thread,
				view: 'chat'
			});
			expect(store.isAdoptedPane(tab.panes[0].id)).toBe(true);
			expect(store.adoptableServerChats([summary])).toEqual([]);
		});
	});
});
