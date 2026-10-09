import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MobileClient } from './client.svelte.ts';
import { WorkspaceRemote } from './remote.svelte.ts';
import {
	CONNECT_ROUTES,
	FakeEventSource,
	jsonResponse,
	routeFetch,
	stubLocalStorage,
	TOKEN,
	type Route
} from './test-helpers.ts';
import type { ServerWorkspace as Workspace, WorkspacePane } from '@workbench/types';

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));

function pane(id: string, extra: Partial<WorkspacePane> = {}): WorkspacePane {
	return {
		id,
		kind: 'claude',
		sessionId: `session-${id}`,
		terminalId: `terminal-${id}`,
		status: 'running',
		title: null,
		busy: false,
		busySince: null,
		turnEndedAt: null,
		running: null,
		waiting: null,
		waitingSince: null,
		error: null,
		generation: 1,
		...extra
	};
}

function workspace(...panes: WorkspacePane[]): Workspace {
	return {
		id: 'w1',
		projectPath: '/repo',
		projectName: 'repo',
		renderer: 'xterm',
		tabs: panes.map((p) => ({
			id: `tab-${p.id}`,
			label: 'Claude 1',
			kind: p.kind,
			split: 'horizontal',
			panes: [p]
		}))
	};
}

describe('MobileClient on a host with the workspace API', () => {
	let sources: FakeEventSource[];
	let commands: Record<string, unknown>[];
	let reply: (cmd: Record<string, unknown>) => Response;

	beforeEach(() => {
		stubLocalStorage();
		vi.stubGlobal('EventSource', FakeEventSource);
		sources = [];
		commands = [];
		reply = () => jsonResponse({ rev: 1, paneId: 'p1' });
	});
	afterEach(() => {
		vi.restoreAllMocks();
		vi.unstubAllGlobals();
	});

	const stream = () => sources[sources.length - 1];
	const snapshot = (rev: number, ...workspaces: Workspace[]) =>
		stream().emit('snapshot', { rev, workspaces });

	async function connected(routes: Record<string, Route> = {}) {
		const fetchSpy = routeFetch({
			...CONNECT_ROUTES,
			'/remote/terminals': () => jsonResponse([]),
			'/projects': () => jsonResponse([{ name: 'repo', path: '/repo' }]),
			'/projects/worktrees': () =>
				jsonResponse([{ path: '/repo-wt', branch: 'feat/x', isMain: false }]),
			'/workspace/commands': (init) => {
				// A newSession's requestId is random: the transport's own tests cover it.
				const { requestId, ...cmd } = JSON.parse(String(init?.body));
				if (cmd.type === 'newSession') expect(requestId).toMatch(/^[0-9a-f]{32}$/);
				commands.push(cmd);
				return reply(cmd);
			},
			...routes
		});
		const c = new MobileClient(undefined, (url) => {
			const source = new FakeEventSource(url);
			sources.push(source);
			return source as unknown as EventSource;
		});
		c.url = 'box:4317';
		c.token = TOKEN;
		await c.connect();
		return { c, fetchSpy };
	}

	it('follows /events/workspace and renders its panes', async () => {
		const { c } = await connected();
		expect(c.remote).toBeInstanceOf(WorkspaceRemote);
		expect(stream().url).toBe(`http://box:4317/events/workspace?token=${TOKEN}`);
		snapshot(1, workspace(pane('p1', { title: 'Fix the build', busy: true })));
		expect(c.panes.map((e) => [e.pane.title, e.pane.busy])).toEqual([['Fix the build', true]]);
	});

	it('a start sends NewSession for the folder and opens the pane the host made', async () => {
		const { c } = await connected();
		await vi.waitFor(() => expect(c.store?.projects).toHaveLength(1));
		await c.start('claude', { projectPath: '/repo' });
		expect(commands).toEqual([
			{ type: 'newSession', projectPath: '/repo', projectName: 'repo', kind: 'claude' }
		]);
		expect(c.screens.openPaneId).toBe('p1');

		snapshot(1, workspace(pane('p1')));
		expect(c.activePane?.pane.id).toBe('p1');
	});

	it('starts Codex as a chat, and a terminal in a worktree with its branch', async () => {
		const { c } = await connected();
		await vi.waitFor(() => expect(c.store?.projects).toHaveLength(1));
		await c.store!.loadWorktrees('/repo');
		await c.start('codex', { projectPath: '/repo' });
		snapshot(1, workspace(pane('p1', { kind: 'codex', codexMode: 'appServer' })));
		await c.start('shell', { projectPath: '/repo', worktreePath: '/repo-wt' });
		expect(commands).toEqual([
			{
				type: 'newSession',
				projectPath: '/repo',
				projectName: 'repo',
				kind: 'codex',
				codexMode: 'appServer'
			},
			{
				type: 'newSession',
				projectPath: '/repo',
				worktreePath: '/repo-wt',
				projectName: 'repo',
				branch: 'feat/x',
				kind: 'shell'
			}
		]);
	});

	it('the history sheet resumes through NewSession, keeping the account', async () => {
		const { c } = await connected();
		await c.resume('claude', { projectPath: '/repo' }, 'old-session', 'work');
		expect(commands).toEqual([
			expect.objectContaining({
				type: 'newSession',
				kind: 'claude',
				resume: 'old-session',
				accountId: 'work'
			})
		]);
	});

	it('closes the screen when its pane leaves the snapshot', async () => {
		const { c } = await connected();
		snapshot(1, workspace(pane('p1'), pane('p2')));
		c.openPane('p1');
		expect(c.activePane?.pane.id).toBe('p1');

		snapshot(2, workspace(pane('p2')));
		expect(c.activePane).toBeNull();
	});

	it('End sends ClosePane; Back sends nothing', async () => {
		const { c, fetchSpy } = await connected();
		snapshot(1, workspace(pane('p1')));
		c.openPane('p1');
		const sent = () =>
			fetchSpy.mock.calls.filter(([url]) => url.includes('/workspace/commands')).length;

		c.closeScreen();
		expect(c.activePane).toBeNull();
		expect(sent()).toBe(0);

		c.openPane('p1');
		await c.endPane('p1');
		expect(commands).toEqual([{ type: 'closePane', paneId: 'p1' }]);
		snapshot(2, workspace());
		expect(c.screens.openPaneId).toBeNull();
	});

	it('shows Starting at once, takes one start at a time, and opens the pane once shown', async () => {
		let answer!: () => void;
		reply = () => jsonResponse({ rev: 1, paneId: 'p1' });
		const { c } = await connected({
			'/workspace/commands': async (init) => {
				commands.push(JSON.parse(String(init?.body)));
				await new Promise<void>((done) => (answer = done));
				return reply({});
			}
		});
		const first = c.start('claude', { projectPath: '/repo' });
		expect(c.screens.starting).toEqual({ kind: 'claude' });
		await c.start('shell', { projectPath: '/repo' });
		expect(commands).toHaveLength(1);

		answer();
		await first;
		expect(c.screens.starting).not.toBeNull();
		expect(c.activePane).toBeNull();
		snapshot(1, workspace(pane('p1')));
		expect(c.screens.starting).toBeNull();
		expect(c.activePane?.pane.id).toBe('p1');
	});

	it('gives up on a start the host never shows', async () => {
		const { c } = await connected();
		vi.useFakeTimers();
		await c.start('claude', { projectPath: '/repo' });
		await vi.advanceTimersByTimeAsync(15_000);
		vi.useRealTimers();
		expect(c.screens.starting).toBeNull();
		expect(c.screens.openPaneId).toBeNull();
		expect(c.notice).toMatch(/did not show up/);
	});

	it('Back during a start drops it, and the late answer opens nothing', async () => {
		let answer!: () => void;
		const { c } = await connected({
			'/workspace/commands': async () => {
				await new Promise<void>((done) => (answer = done));
				return jsonResponse({ rev: 1, paneId: 'p1' });
			}
		});
		const starting = c.start('shell', { projectPath: '/repo' });
		c.closeScreen();
		await vi.waitFor(() => expect(answer).toBeTypeOf('function'));
		answer();
		await starting;
		snapshot(1, workspace(pane('p1', { kind: 'shell' })));
		expect(c.activePane).toBeNull();
	});

	it("shows the host's account, switches it on the host, and lets the host pick a new chat's", async () => {
		let active: string | null = 'work';
		const puts: unknown[] = [];
		const { c } = await connected({
			'/settings/workbench': () =>
				jsonResponse({
					claudeAccounts: [{ id: 'work', name: 'Work', configDir: '/account' }],
					activeClaudeAccount: active
				}),
			'/settings/active-claude-account': (init) => {
				const body = JSON.parse(String(init?.body));
				puts.push(body);
				active = body.id;
				return new Response(null, { status: 204 });
			}
		});
		await vi.waitFor(() => expect(c.accountId).toBe('work'));
		await c.start('claude', { projectPath: '/repo' });
		expect(commands[0]).not.toHaveProperty('accountId');

		await c.setAccount('');
		expect(puts).toEqual([{ id: null }]);
		expect(c.accountId).toBeUndefined();
		active = 'work';
		c.refreshAll();
		await vi.waitFor(() => expect(c.accountId).toBe('work'));
	});

	it("shows a spawn's notice once", async () => {
		const { c } = await connected();
		snapshot(1, workspace(pane('p1', { notice: 'Prompt left out' })));
		expect(c.notice).toBe('Prompt left out');
		c.notice = null;
		snapshot(2, workspace(pane('p1', { notice: 'Prompt left out', busy: true })));
		expect(c.notice).toBeNull();
		snapshot(3, workspace(pane('p1', { notice: 'Prompt left out', generation: 2 })));
		expect(c.notice).toBe('Prompt left out');
	});

	it("a chat's account is the pane's, from the host", async () => {
		const { c } = await connected();
		snapshot(1, workspace(pane('p1', { accountId: 'work' })));
		expect(c.chatRef(c.panes[0]).claudeAccountId).toBe('work');
	});

	it('Restart and Trust are commands on the tab and pane', async () => {
		const { c } = await connected();
		await c.restart('tab-p1');
		await c.trustFolder('p1');
		expect(commands).toEqual([
			{ type: 'restart', tabId: 'tab-p1' },
			{ type: 'trustFolder', paneId: 'p1' }
		]);
	});

	it('shows a refused command as a notice', async () => {
		reply = () => jsonResponse({ rev: 3, error: 'no such tab' }, 400);
		const { c } = await connected();
		await c.restart('gone');
		expect(c.notice).toBe("Couldn't restart the session: no such tab");
	});

	it('Chat or Terminal for a Claude pane is local; Codex follows its process', async () => {
		const { c } = await connected();
		const codexTui = pane('p2', { kind: 'codex', codexMode: 'tui' });
		snapshot(1, workspace(pane('p1'), codexTui));
		expect(c.paneView(c.panes[0].pane)).toBe('chat');
		c.setView('p1', 'terminal');
		expect(c.paneView(c.panes[0].pane)).toBe('terminal');
		expect(c.paneView(codexTui)).toBe('terminal');
		expect(commands).toEqual([]);
	});

	it('ignores a stale rev, but takes a new connection’s first snapshot', async () => {
		const { c } = await connected();
		snapshot(5, workspace(pane('p1')));
		snapshot(4, workspace());
		expect(c.panes).toHaveLength(1);

		// A restarted host counts from the start again.
		vi.useFakeTimers();
		stream().emit('error');
		await vi.advanceTimersByTimeAsync(1000);
		vi.useRealTimers();
		expect(sources).toHaveLength(2);
		snapshot(1, workspace());
		expect(c.panes).toHaveLength(0);
	});

	it('a notification opens the pane holding its session, also after /clear', async () => {
		const { c } = await connected();
		snapshot(1, workspace(pane('p1', { sessionId: 'new', previousIds: ['old'] })));
		await c.openNotification({
			agent: 'claude',
			sessionId: 'old',
			projectPath: '/repo',
			worktreePath: null,
			title: null,
			claudeAccountId: null
		});
		expect(c.screens.openPaneId).toBe('p1');
		expect(c.paneView(c.activePane!.pane)).toBe('chat');
	});
});

describe('MobileClient on an older host', () => {
	beforeEach(() => stubLocalStorage());
	afterEach(() => vi.unstubAllGlobals());

	it('asks for an update when /health has no workspaceApi', async () => {
		routeFetch({ ...CONNECT_ROUTES, '/health': () => jsonResponse({ ok: true }) });
		const c = new MobileClient();
		c.url = 'box:4317';
		c.token = TOKEN;
		await c.connect();
		expect(c.remote).toBeNull();
		expect(c.connectError).toMatch(/^Update Workbench on your computer/);
	});
});
