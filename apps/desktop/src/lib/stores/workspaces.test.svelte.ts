import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createMockTransport, type MockTransport } from '@workbench/transport';
import type {
	ServerWorkspace,
	WorkspacePane,
	WorkspaceSnapshot,
	WorkspaceTab
} from '$types/workspace';
import { WorkspaceStore } from './workspaces.svelte';
import { UI_KEY } from './workspace-ui';
import { releaseChat } from '$features/chat/chat-registry';

vi.mock('$features/chat/chat-registry', () => ({ releaseChat: vi.fn() }));
vi.mock('svelte-sonner', () => ({ toast: { error: vi.fn() } }));

const mockGitStore = {
	branchByProject: {} as Record<string, string>,
	statusByProject: {} as Record<string, { branch: string }>
};
const mockSettings = {
	defaultClaudeView: 'terminal' as 'terminal' | 'chat',
	terminalRenderer: 'xterm' as const
};
vi.mock('./context', () => ({
	getGitStore: () => mockGitStore,
	getWorkbenchSettingsStore: () => mockSettings
}));

/** Object-backed localStorage (this jsdom env's lacks `clear`). */
let storage: Record<string, string>;
function stubStorage() {
	storage = {};
	vi.stubGlobal('localStorage', {
		getItem: (k: string) => storage[k] ?? null,
		setItem: (k: string, v: string) => void (storage[k] = v),
		removeItem: (k: string) => void delete storage[k]
	});
}
const savedUi = () => JSON.parse(storage[UI_KEY] ?? 'null');

function pane(id: string, over: Partial<WorkspacePane> = {}): WorkspacePane {
	return {
		id,
		kind: 'shell',
		terminalId: `term-${id}`,
		status: 'running',
		title: null,
		busy: false,
		waiting: null,
		error: null,
		generation: 1,
		...over
	};
}

function tab(id: string, panes: WorkspacePane[], over: Partial<WorkspaceTab> = {}): WorkspaceTab {
	return { id, label: id, kind: panes[0]?.kind ?? 'shell', split: 'horizontal', panes, ...over };
}

function ws(
	id: string,
	tabs: WorkspaceTab[],
	over: Partial<ServerWorkspace> = {}
): ServerWorkspace {
	return {
		id,
		projectPath: '/repo',
		projectName: 'Repo',
		renderer: 'xterm',
		tabs,
		...over
	};
}

const snap = (rev: number, workspaces: ServerWorkspace[]): WorkspaceSnapshot => ({
	rev,
	workspaces
});

describe('WorkspaceStore', () => {
	let transport: MockTransport;
	let store: WorkspaceStore;

	beforeEach(() => {
		stubStorage();
		mockSettings.defaultClaudeView = 'terminal';
		transport = createMockTransport();
		store = new WorkspaceStore(transport);
	});

	afterEach(() => {
		store.dispose();
		vi.unstubAllGlobals();
		vi.clearAllMocks();
	});

	/** Load, then deliver the first snapshot. */
	async function loaded(first: WorkspaceSnapshot) {
		const ready = store.load();
		transport.emitWorkspace(first, true);
		await ready;
	}

	describe('snapshots', () => {
		it('renders workspaces → tabs → panes from the snapshot', async () => {
			await loaded(
				snap(1, [
					ws('w1', [
						tab('t1', [pane('p1')]),
						tab('t2', [
							pane('p2', { kind: 'claude', sessionId: 's2', accountId: 'work', busy: true })
						])
					])
				])
			);
			const [w] = store.workspaces;
			expect(w.terminalTabs.map((t) => [t.id, t.type])).toEqual([
				['t1', 'shell'],
				['t2', 'claude']
			]);
			expect(w.activeTerminalTabId).toBe('t1');
			expect(w.terminalTabs[1].panes[0]).toMatchObject({
				id: 'p2',
				claudeSessionId: 's2',
				claudeAccountId: 'work',
				terminalId: 'term-p2',
				busy: true
			});
			expect(store.activeWorkspace?.id).toBe('w1');
			expect(store.activeTerminalTab?.id).toBe('t1');
		});

		it("labels an AI tab with its session's title once it has one", async () => {
			await loaded(
				snap(1, [
					ws('w1', [
						tab('t1', [pane('p1', { kind: 'claude', title: 'Fix the build' })], {
							label: 'Claude 1'
						}),
						tab('t2', [pane('p2', { title: 'ignored' })], { label: 'Terminal 1' })
					])
				])
			);
			expect(store.workspaces[0].terminalTabs.map((t) => t.label)).toEqual([
				'Fix the build',
				'Terminal 1'
			]);
		});

		it("shows a Codex pane's mode as its view", async () => {
			await loaded(
				snap(1, [
					ws('w1', [
						tab('t1', [pane('p1', { kind: 'codex', codexMode: 'appServer', sessionId: 'th' })])
					])
				])
			);
			expect(store.pane('p1')?.view).toBe('chat');
		});

		it('ignores a frame whose rev is not newer, but takes any rev on a new connection', async () => {
			await loaded(snap(5, [ws('w1', [])]));
			transport.emitWorkspace(snap(5, [ws('w2', [])]));
			transport.emitWorkspace(snap(4, [ws('w3', [])]));
			expect(store.workspaces.map((w) => w.id)).toEqual(['w1']);
			transport.emitWorkspace(snap(6, [ws('w4', [])]));
			expect(store.workspaces.map((w) => w.id)).toEqual(['w4']);
			transport.emitWorkspace(snap(1, [ws('w5', [])]), true);
			expect(store.workspaces.map((w) => w.id)).toEqual(['w5']);
		});

		it('releases the chat of a pane that left the snapshot', async () => {
			await loaded(snap(1, [ws('w1', [tab('t1', [pane('p1'), pane('p2')])])]));
			transport.emitWorkspace(snap(2, [ws('w1', [tab('t1', [pane('p1')])])]));
			expect(releaseChat).toHaveBeenCalledExactlyOnceWith('p2');
		});
	});

	describe('local state', () => {
		it('keeps selection, active tab and Claude view per id in localStorage', async () => {
			const two = [
				ws('w1', [tab('t1', [pane('p1')])]),
				ws('w2', [
					tab('t2', [pane('p2', { kind: 'claude', sessionId: 's' })]),
					tab('t3', [pane('p3')])
				])
			];
			await loaded(snap(1, two));
			store.selectedId = 'w2';
			store.setActiveTab('w2', 't3');
			await store.setPaneView('p2', 'chat');

			expect(savedUi()).toEqual({
				selectedId: 'w2',
				activeTabs: { w2: 't3' },
				chatPanes: ['p2']
			});
			expect(transport.workspaceCommands).toEqual([]);

			store.dispose();
			const again = new WorkspaceStore(transport);
			const ready = again.load();
			transport.emitWorkspace(snap(2, two), true);
			await ready;
			expect(again.activeWorkspaceId).toBe('w2');
			expect(again.activeTerminalTab?.id).toBe('t3');
			expect(again.pane('p2')?.view).toBe('chat');
			again.dispose();
		});

		it('prunes keys whose ids left the snapshot', async () => {
			await loaded(
				snap(1, [
					ws('w1', [tab('t1', [pane('p1', { kind: 'claude', sessionId: 's' })])]),
					ws('w2', [tab('t2', [pane('p2')])])
				])
			);
			store.selectedId = 'w2';
			store.setActiveTab('w1', 't1');
			await store.setPaneView('p1', 'chat');

			transport.emitWorkspace(snap(2, [ws('w3', [])]));
			expect(savedUi()).toEqual({ selectedId: null, activeTabs: {}, chatPanes: [] });
		});

		it('survives storage that throws', async () => {
			vi.stubGlobal('localStorage', {
				getItem: () => {
					throw new Error('blocked');
				},
				setItem: () => {
					throw new Error('blocked');
				}
			});
			const s = new WorkspaceStore(transport);
			const ready = s.load();
			transport.emitWorkspace(snap(1, [ws('w1', [tab('t1', [pane('p1')])])]), true);
			await ready;
			s.setActiveTab('w1', 't1');
			expect(s.activeTerminalTab?.id).toBe('t1');
			s.dispose();
		});

		it('shows what a command opened, even before a snapshot includes it', async () => {
			await loaded(snap(1, [ws('w1', [tab('t1', [pane('p1')])])]));
			transport.mockWorkspaceCommand(() => ({
				rev: 3,
				workspaceId: 'w1',
				tabId: 't9',
				paneId: 'p9'
			}));
			await store.addTerminalTab('w1');
			// A snapshot older than the command's doesn't prune the new tab's selection.
			transport.emitWorkspace(snap(2, [ws('w1', [tab('t1', [pane('p1')])])]));
			expect(savedUi().activeTabs).toEqual({ w1: 't9' });
			transport.emitWorkspace(
				snap(3, [ws('w1', [tab('t1', [pane('p1')]), tab('t9', [pane('p9')])])])
			);
			expect(store.activeTerminalTab?.id).toBe('t9');
		});

		it('takes over the state an older desktop saved, once, when this device has none', async () => {
			const first = snap(1, [
				ws('w1', [tab('t1', [pane('p1')])]),
				ws('w2', [
					tab('t2', [pane('p2', { kind: 'claude', sessionId: 's' })]),
					tab('t3', [pane('p3')])
				])
			]);
			await loaded({
				...first,
				local: { selectedId: 'w2', activeTabIds: { w2: 't3' }, chatPanes: ['p2'] }
			});
			expect(store.activeWorkspaceId).toBe('w2');
			expect(store.activeTerminalTab?.id).toBe('t3');
			expect(store.pane('p2')?.view).toBe('chat');

			// Later frames carry it too, but this device's own choices now win.
			store.selectedId = 'w1';
			transport.emitWorkspace({ ...first, rev: 2, local: { selectedId: 'w2' } });
			expect(store.activeWorkspaceId).toBe('w1');
		});

		it('never seeds over state this device saved', async () => {
			storage[UI_KEY] = JSON.stringify({ selectedId: 'w1', activeTabs: {}, chatPanes: [] });
			const s2 = new WorkspaceStore(transport);
			const ready = s2.load();
			transport.emitWorkspace(
				{ ...snap(1, [ws('w1', []), ws('w2', [])]), local: { selectedId: 'w2' } },
				true
			);
			await ready;
			expect(s2.activeWorkspaceId).toBe('w1');
			s2.dispose();
		});

		it("reports the server's persistence, ok when it doesn't say", async () => {
			await loaded(snap(1, []));
			expect(store.persistence.status).toBe('ok');
			transport.emitWorkspace({
				...snap(2, []),
				persistence: { status: 'locked', message: 'Another Workbench server (pid 7)' }
			});
			expect(store.persistence).toEqual({
				status: 'locked',
				message: 'Another Workbench server (pid 7)'
			});
		});

		it('keeps a workspace a command opened selected until a snapshot shows it', async () => {
			await loaded(snap(1, [ws('w1', [])]));
			transport.mockWorkspaceCommand(() => ({ rev: 3, workspaceId: 'w9' }));
			await store.open({ name: 'Repo', path: '/repo' });
			transport.emitWorkspace(snap(2, [ws('w1', [])]));
			expect(savedUi().selectedId).toBe('w9');
			transport.emitWorkspace(snap(3, [ws('w1', []), ws('w9', [])]));
			expect(store.activeWorkspaceId).toBe('w9');
		});

		it('opens a new Claude tab as chat when that is the default', async () => {
			mockSettings.defaultClaudeView = 'chat';
			await loaded(snap(1, [ws('w1', [])]));
			transport.mockWorkspaceCommand(() => ({
				rev: 2,
				workspaceId: 'w1',
				tabId: 't1',
				paneId: 'p1'
			}));
			await store.addAISession('w1', 'claude');
			expect(savedUi().chatPanes).toEqual(['p1']);
		});
	});

	describe('commands', () => {
		const project = { name: 'Repo', path: '/repo' };

		beforeEach(async () => {
			await loaded(
				snap(1, [
					ws('w1', [
						tab('t1', [pane('p1')]),
						tab('t2', [pane('p2', { kind: 'codex', codexMode: 'tui', sessionId: 'th' })])
					])
				])
			);
		});

		it.each([
			[
				'open',
				(s: WorkspaceStore) => s.open(project),
				{ type: 'openWorkspace', projectPath: '/repo', projectName: 'Repo', renderer: 'xterm' }
			],
			[
				'openWorktree',
				(s: WorkspaceStore) => s.openWorktree(project, '/repo-wt', 'feat'),
				{
					type: 'openWorkspace',
					projectPath: '/repo',
					projectName: 'Repo',
					worktreePath: '/repo-wt',
					branch: 'feat',
					renderer: 'xterm'
				}
			],
			[
				'close',
				(s: WorkspaceStore) => s.close('w1'),
				{ type: 'closeWorkspace', workspaceId: 'w1' }
			],
			[
				'closeAllForProject',
				(s: WorkspaceStore) => s.closeAllForProject('/repo'),
				{ type: 'closeProject', projectPath: '/repo' }
			],
			[
				'addTerminalTab',
				(s: WorkspaceStore) => s.addTerminalTab('w1'),
				{ type: 'newSession', workspaceId: 'w1', kind: 'shell' }
			],
			[
				'runTaskInWorkspace',
				(s: WorkspaceStore) =>
					s.runTaskInWorkspace('w1', { name: 'Login', command: 'claude auth login' }, 'work'),
				{
					type: 'newSession',
					workspaceId: 'w1',
					kind: 'shell',
					label: 'Login',
					command: 'claude auth login',
					accountId: 'work'
				}
			],
			[
				'addAISession (claude)',
				(s: WorkspaceStore) => s.addAISession('w1', 'claude'),
				{ type: 'newSession', workspaceId: 'w1', kind: 'claude' }
			],
			[
				'addAISession (agent action)',
				(s: WorkspaceStore) => s.addAISession('w1', 'codex', { label: 'Review', prompt: ' Go ' }),
				{ type: 'newSession', workspaceId: 'w1', kind: 'codex', label: 'Review', prompt: 'Go' }
			],
			[
				'addAIByProject',
				(s: WorkspaceStore) => s.addAIByProject(project, 'claude'),
				{ type: 'newSession', projectPath: '/repo', projectName: 'Repo', kind: 'claude' }
			],
			[
				'resumeAISession',
				(s: WorkspaceStore) => s.resumeAISession('w1', 'sess-9', 'Old chat', 'claude', 'work'),
				{
					type: 'newSession',
					workspaceId: 'w1',
					kind: 'claude',
					resume: 'sess-9',
					label: 'Old chat',
					accountId: 'work'
				}
			],
			[
				'resumeAISession (codex, chat)',
				(s: WorkspaceStore) =>
					s.resumeAISession('w1', 'th-2', 'Thread', 'codex', undefined, 'chat'),
				{
					type: 'newSession',
					workspaceId: 'w1',
					kind: 'codex',
					resume: 'th-2',
					label: 'Thread',
					codexMode: 'appServer'
				}
			],
			[
				'closeTerminalTab',
				(s: WorkspaceStore) => s.closeTerminalTab('w1', 't1'),
				{ type: 'closeTab', tabId: 't1' }
			],
			[
				'removePane',
				(s: WorkspaceStore) => s.removePane('w1', 'p1'),
				{ type: 'closePane', paneId: 'p1' }
			],
			[
				'restartAISession',
				(s: WorkspaceStore) => s.restartAISession('w1', 't2'),
				{ type: 'restart', tabId: 't2' }
			],
			[
				'setPaneView (codex)',
				(s: WorkspaceStore) => s.setPaneView('p2', 'chat'),
				{ type: 'setCodexMode', paneId: 'p2', mode: 'appServer' }
			],
			[
				'renameTab',
				(s: WorkspaceStore) => s.renameTab('t1', 'Build'),
				{ type: 'rename', tabId: 't1', label: 'Build' }
			],
			[
				'splitTerminal',
				(s: WorkspaceStore) => s.splitTerminal('w1', 'vertical'),
				{ type: 'split', tabId: 't1', direction: 'vertical' }
			],
			[
				'reorderTerminalTab',
				(s: WorkspaceStore) => s.reorderTerminalTab('w1', 't2', 't1'),
				{ type: 'moveTab', tabId: 't2', toTabId: 't1' }
			],
			[
				'reorder',
				(s: WorkspaceStore) => s.reorder('w1', 'w2'),
				{ type: 'moveWorkspace', workspaceId: 'w1', toWorkspaceId: 'w2' }
			],
			[
				'trustFolder',
				(s: WorkspaceStore) => s.trustFolder('p1'),
				{ type: 'trustFolder', paneId: 'p1' }
			],
			[
				'updateProject',
				(s: WorkspaceStore) => s.updateProject('/repo', { path: '/moved', name: 'Moved' }),
				{ type: 'updateProject', projectPath: '/repo', newPath: '/moved', projectName: 'Moved' }
			]
		])('%s sends exactly one command', async (_name, act, command) => {
			await act(store);
			expect(transport.workspaceCommands).toEqual([command]);
		});

		it('shows a refused command instead of throwing', async () => {
			const { toast } = await import('svelte-sonner');
			transport.mockWorkspaceCommand(() => {
				throw new Error("Native terminals can't be split");
			});
			await expect(store.splitTerminal('w1', 'horizontal')).resolves.toBeNull();
			expect(toast.error).toHaveBeenCalledWith("Native terminals can't be split");
		});

		it('selects the workspace a command opened', async () => {
			transport.mockWorkspaceCommand(() => ({ rev: 2, workspaceId: 'w7' }));
			await store.open(project);
			expect(store.selectedId).toBe('w7');
		});
	});
});
