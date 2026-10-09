import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentSummary } from '@workbench/types';
import { MobileClient } from './client.svelte.ts';
import { LegacyRemote, legacyWorkspaces } from './legacy-remote.svelte.ts';
import {
	CONNECT_ROUTES,
	jsonResponse,
	routeFetch,
	stubLocalStorage,
	TOKEN
} from './test-helpers.ts';

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));

const chat: AgentSummary = {
	agent: 'claude',
	sessionId: 's1',
	projectPath: '/repo',
	worktreePath: null,
	paneId: null,
	claudeAccountId: 'work',
	title: 'Fix the build',
	model: null,
	busy: false,
	exited: false,
	busySince: null,
	updatedAt: 0,
	waiting: null,
	running: null,
	previousIds: [],
	terminalId: 't1'
};
const terminal = { id: 't1', cwd: '/repo', createdAt: 0, alive: true, claudeSessionId: 's1' };

describe('legacyWorkspaces', () => {
	it('shows a chat and its backing terminal as one pane', () => {
		const panes = legacyWorkspaces([chat], [terminal]).flatMap((w) =>
			w.tabs.flatMap((t) => t.panes)
		);
		expect(panes).toEqual([
			expect.objectContaining({
				id: 'claude:s1',
				kind: 'claude',
				sessionId: 's1',
				terminalId: 't1',
				accountId: 'work',
				status: 'running'
			})
		]);
	});

	it('keeps one pane id before the plugin attaches and across /clear', () => {
		const before = legacyWorkspaces([], [terminal])[0].tabs[0].panes[0];
		expect(before).toMatchObject({ id: 'claude:s1', status: 'starting', terminalId: 't1' });
		const cleared = { ...chat, sessionId: 's2', previousIds: ['s1'] };
		expect(legacyWorkspaces([cleared], [terminal])[0].tabs[0].panes[0].id).toBe('claude:s1');
	});

	it('leaves out exited chats, and shows a plain terminal as a shell', () => {
		const shell = { id: 't2', cwd: '/repo', createdAt: 0, alive: false };
		const panes = legacyWorkspaces(
			[{ ...chat, exited: true, terminalId: undefined }],
			[shell]
		).flatMap((w) => w.tabs.flatMap((t) => t.panes));
		expect(panes).toEqual([
			expect.objectContaining({ id: 'term:t2', kind: 'shell', status: 'exited' })
		]);
	});
});

describe('MobileClient commands on an older host', () => {
	beforeEach(() => stubLocalStorage());
	afterEach(() => {
		vi.restoreAllMocks();
		vi.unstubAllGlobals();
	});

	async function connected(terminals: unknown[] = [], chats: unknown[] = []) {
		const fetchSpy = routeFetch({
			...CONNECT_ROUTES,
			'/remote/terminals': (init) =>
				jsonResponse(
					init?.method === 'POST'
						? { id: 't9', cwd: '/repo', createdAt: 0, alive: true }
						: terminals
				),
			'/agent': () => jsonResponse(chats),
			'/agent/claude/s1': () => new Response(null, { status: 204 })
		});
		const c = new MobileClient();
		c.url = 'box:4317';
		c.token = TOKEN;
		await c.connect();
		expect(c.remote).toBeInstanceOf(LegacyRemote);
		const sent = () =>
			fetchSpy.mock.calls
				.filter(([, init]) => init?.method && init.method !== 'GET')
				.map(([url, init]) => ({
					call: `${init!.method} ${new URL(url).pathname}${new URL(url).search}`,
					body: init!.body ? JSON.parse(String(init!.body)) : undefined
				}));
		return { c, sent };
	}

	it('a Claude start opens a terminal running claude on a new session', async () => {
		const { c, sent } = await connected();
		await c.start('claude', { projectPath: '/repo' }, 'terminal');
		const [create] = sent();
		expect(create.call).toBe('POST /remote/terminals');
		expect(create.body.claudeSession.id).toMatch(/^[0-9a-f-]{36}$/);
		expect(c.activePane?.pane).toMatchObject({
			kind: 'claude',
			terminalId: 't9',
			sessionId: create.body.claudeSession.id
		});
	});

	it('End on a chat ends its session everywhere', async () => {
		const { c, sent } = await connected([terminal], [chat]);
		await c.endPane('claude:s1');
		expect(sent().map((s) => s.call)).toEqual(['DELETE /agent/claude/s1?end=true']);
	});

	it('a Codex terminal notification attaches its terminal, never starting a chat', async () => {
		const { c, sent } = await connected([{ id: 't1', cwd: '/p', createdAt: 0, alive: true }]);
		await c.openNotification({
			agent: 'codex',
			sessionId: 'thread',
			projectPath: '/p',
			worktreePath: null,
			title: null,
			claudeAccountId: null,
			terminalOnly: true,
			terminalId: 't1'
		});
		expect(c.activePane?.pane.terminalId).toBe('t1');
		expect(sent()).toEqual([]);
	});

	it('a terminal that is only on the desktop keeps Home open with a notice', async () => {
		const { c } = await connected();
		await c.openNotification({
			agent: 'codex',
			sessionId: 'thread',
			projectPath: '/p',
			worktreePath: null,
			title: null,
			claudeAccountId: null,
			terminalOnly: true
		});
		expect(c.activePane).toBeNull();
		expect(c.notice).toContain('desktop');
	});
});
