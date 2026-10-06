import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MobileClient, openExternal } from './client.svelte.ts';
import { CAMERA_DENIED, NOT_A_PAIRING_CODE, type QrScanner } from './qr-scan.svelte.ts';
import {
	CONNECT_ROUTES,
	jsonResponse,
	routeFetch,
	stubLocalStorage,
	TOKEN,
	type Route
} from './test-helpers.ts';
import { buildPairingUri } from '@workbench/transport';
import { openUrl } from '@tauri-apps/plugin-opener';

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));

describe('openExternal', () => {
	it('logs a failed open instead of throwing', async () => {
		const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
		vi.mocked(openUrl).mockRejectedValueOnce(new Error('denied'));
		expect(() => openExternal('https://github.com/o/r')).not.toThrow();
		await vi.waitFor(() => expect(warn).toHaveBeenCalled());
		expect(openUrl).toHaveBeenCalledWith('https://github.com/o/r');
		warn.mockRestore();
	});
});

describe('MobileClient', () => {
	beforeEach(() => stubLocalStorage());
	afterEach(() => {
		vi.restoreAllMocks();
		vi.unstubAllGlobals();
	});

	async function connected(extra: Record<string, Route> = {}) {
		routeFetch({ ...CONNECT_ROUTES, '/remote/terminals': () => jsonResponse([]), ...extra });
		const c = new MobileClient();
		c.url = 'box:4317';
		c.token = TOKEN;
		await c.connect();
		return c;
	}

	it('a Codex terminal notification attaches its existing terminal without creating a chat', async () => {
		const c = await connected({
			'/remote/terminals': () => jsonResponse([{ id: 't1', cwd: '/p', createdAt: 0, alive: true }])
		});
		const start = vi.spyOn(c.agents, 'start');
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
		expect(c.activeTerminalId).toBe('t1');
		expect(c.activeChat).toBeNull();
		expect(start).not.toHaveBeenCalled();
	});

	it('a desktop-only Codex terminal notification keeps Home open rather than starting another process', async () => {
		const c = await connected();
		await c.openNotification({
			agent: 'codex',
			sessionId: 'thread',
			projectPath: '/p',
			worktreePath: null,
			title: null,
			claudeAccountId: null,
			terminalOnly: true
		});
		expect(c.activeChat).toBeNull();
		expect(c.activeTerminalId).toBeNull();
		expect(c.notice).toContain('desktop');
	});

	it('connect() normalizes the url, sets the store, and loads terminals', async () => {
		const c = await connected({
			'/remote/terminals': () => jsonResponse([{ id: 't1', cwd: '/p', createdAt: 0, alive: true }])
		});
		expect(c.connectError).toBeNull();
		expect(c.store).not.toBeNull();
		expect(c.url).toBe('http://box:4317');
		expect(c.terminals).toHaveLength(1);
	});

	it('connect() refuses to connect without a token', async () => {
		const fetchSpy = routeFetch(CONNECT_ROUTES);
		const c = new MobileClient();
		c.url = 'box:4317';
		c.token = '   ';
		await c.connect();
		expect(c.store).toBeNull();
		expect(c.connectError).toBe('enter the server token');
		expect(fetchSpy).not.toHaveBeenCalled();
		expect(c.machines.list).toEqual([]);
	});

	it('connect() reports an invalid token and does not save it', async () => {
		routeFetch({
			...CONNECT_ROUTES,
			'/remote/terminals': (init) =>
				(init?.headers as Record<string, string>)?.authorization === `Bearer ${TOKEN}`
					? jsonResponse([])
					: jsonResponse('unauthorized', 401)
		});
		const c = new MobileClient();
		c.url = 'box:4317';
		c.token = 'wrong';
		await c.connect();
		expect(c.store).toBeNull();
		expect(c.connectError).toBe('invalid token');
		expect(localStorage.getItem('wb.machines')).toBeNull();
	});

	it('connect() saves the url and token and sends the token on requests', async () => {
		const c = await connected({
			'/remote/terminals': (init) => {
				expect((init?.headers as Record<string, string>)?.authorization).toBe(`Bearer ${TOKEN}`);
				return jsonResponse([]);
			}
		});
		expect(c.connectError).toBeNull();
		expect(c.machines.active).toMatchObject({ url: 'http://box:4317', token: TOKEN });
	});

	it('connect() records an error and leaves the store null on a failed health check', async () => {
		routeFetch({ '/health': () => jsonResponse('no', 503) });
		const c = new MobileClient();
		c.url = 'box:4317';
		c.token = TOKEN;
		await c.connect();
		expect(c.store).toBeNull();
		expect(c.connectError).toMatch(/503/);
	});

	it('connect() gives up on a machine that does not answer', async () => {
		const fetchSpy = vi.fn((_input: string, init?: RequestInit) => {
			expect(init?.signal).toBeInstanceOf(AbortSignal);
			return Promise.reject(new DOMException('The operation timed out.', 'TimeoutError'));
		});
		vi.stubGlobal('fetch', fetchSpy);
		const c = new MobileClient();
		c.url = 'box:4317';
		c.token = TOKEN;
		await c.connect();
		expect(fetchSpy).toHaveBeenCalledTimes(1);
		expect(c.connectError).toBe('not responding (timed out)');
		expect(c.connecting).toBe(false);
	});

	it('refreshTerminals() guards a non-array body instead of throwing', async () => {
		const c = await connected({ '/remote/terminals': () => jsonResponse({ not: 'an array' }) });
		expect(c.terminals).toEqual([]);
	});

	it('createTerminal() opens the new terminal once the server lists it', async () => {
		const meta = { id: 'new-1', cwd: '/p', createdAt: 0, alive: true };
		const c = await connected();
		// POST create → meta; subsequent GET list → [meta].
		vi.stubGlobal(
			'fetch',
			vi.fn((_input: string, init?: RequestInit) =>
				Promise.resolve(jsonResponse(init?.method === 'POST' ? meta : [meta]))
			)
		);
		await c.createTerminal('/p', undefined, 'shell');
		expect(c.activeTerminalId).toBe('new-1');
		expect(c.terminals.map((t) => t.id)).toContain('new-1');
	});

	it('createTerminal() keeps the new terminal open even if the list has not surfaced it', async () => {
		const meta = { id: 'new-2', cwd: '/p', createdAt: 0, alive: true };
		const c = await connected();
		// POST create → meta; GET list stays empty (eventual-consistency race).
		vi.stubGlobal(
			'fetch',
			vi.fn((_input: string, init?: RequestInit) =>
				Promise.resolve(jsonResponse(init?.method === 'POST' ? meta : []))
			)
		);
		await c.createTerminal('/p', undefined, 'shell');
		expect(c.activeTerminalId).toBe('new-2');
		// The view gates on the $derived activeTerminal = terminals.find(id===activeId),
		// so the terminal must remain in `terminals` after the (empty) refresh, otherwise
		// the view never opens. This is the actual fix — assert it, not just the id.
		expect(c.terminals.map((t) => t.id)).toContain('new-2');
	});

	it('disconnect() disposes the store and clears terminal state', async () => {
		const c = await connected({
			'/remote/terminals': () => jsonResponse([{ id: 't1', cwd: '/p', createdAt: 0, alive: true }])
		});
		const store = c.store;
		expect(store).not.toBeNull();
		const disposeSpy = vi.spyOn(store!, 'dispose');
		c.selectTerminal('t1');

		c.disconnect();

		expect(disposeSpy).toHaveBeenCalled();
		expect(c.store).toBeNull();
		expect(c.terminals).toEqual([]);
		expect(c.activeTerminalId).toBeNull();
	});

	it('killTerminal() clears the active id when it matches the killed terminal', async () => {
		const c = await connected();
		c.selectTerminal('t1');
		await c.killTerminal('t1');
		expect(c.activeTerminalId).toBeNull();
	});

	describe('chat sessions', () => {
		const SID = '4d6f2b1e-3c4a-4b5d-8e9f-a0b1c2d3e4f5';
		const summary = {
			agent: 'claude' as const,
			sessionId: SID,
			projectPath: '/repo',
			worktreePath: '/repo-wt',
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
			previousIds: []
		};

		const codexSummary = {
			...summary,
			agent: 'codex' as const,
			sessionId: 'thread-1',
			worktreePath: null,
			claudeAccountId: null,
			title: 'Tidy the docs'
		};

		/** A fake server recording each call; terminals it creates are listed until killed. */
		function fakeServer({ failKill = false, withTerminal = false, failAttach = false } = {}) {
			const calls: { method: string; path: string; body: unknown }[] = [];
			const terminals: {
				id: string;
				name: string;
				cwd: string;
				createdAt: number;
				alive: boolean;
			}[] = withTerminal
				? [{ id: 't1', name: 'Claude', cwd: '/repo-wt', createdAt: 0, alive: true }]
				: [];
			vi.stubGlobal(
				'fetch',
				vi.fn((input: string, init?: RequestInit) => {
					const method = init?.method ?? 'GET';
					const url = new URL(input);
					const path = url.pathname + (url.search || '');
					const body = init?.body ? JSON.parse(String(init.body)) : undefined;
					calls.push({ method, path, body });
					if (path === '/agent' && method === 'GET')
						return Promise.resolve(
							jsonResponse([
								withTerminal ? { ...summary, terminalId: 't1' } : summary,
								codexSummary
							])
						);
					if (path === '/agent/claude' && method === 'POST')
						return Promise.resolve(
							failAttach
								? jsonResponse({ error: 'Not attached' }, 404)
								: jsonResponse({ sessionId: SID, terminalId: 't1' })
						);
					if (path === '/remote/terminals' && method === 'POST') {
						const meta = {
							id: `t${terminals.length + 1}`,
							name: body.name,
							cwd: '/repo',
							createdAt: 0,
							alive: true
						};
						terminals.push(meta);
						return Promise.resolve(jsonResponse(meta));
					}
					if (path === '/remote/terminals') return Promise.resolve(jsonResponse(terminals));
					if (path.startsWith('/remote/terminals/') && method === 'DELETE') {
						if (failKill) return Promise.resolve(new Response(null, { status: 500 }));
						terminals.splice(
							terminals.findIndex((t) => path.endsWith(t.id)),
							1
						);
						return Promise.resolve(new Response(null, { status: 204 }));
					}
					if (method === 'DELETE' || path.endsWith('/message'))
						return Promise.resolve(new Response(null, { status: 204 }));
					return Promise.resolve(jsonResponse(null));
				})
			);
			return calls;
		}

		it('lists running chats with the token', async () => {
			const c = await connected();
			fakeServer();
			await c.refreshChats();
			expect(c.chats.map((s) => s.title)).toEqual(['Fix the build', 'Tidy the docs']);
			const [, init] = vi.mocked(fetch).mock.calls[0] as [string, RequestInit];
			expect((init.headers as Record<string, string>).authorization).toBe(`Bearer ${TOKEN}`);
		});

		it('opens new sessions as chat by default', async () => {
			const c = await connected();
			await c.startClaude('/repo', undefined, 'repo');
			expect(c.activeChat?.projectPath).toBe('/repo');
			expect(c.activeChat?.sessionId).toMatch(/^[0-9a-f-]{36}$/);
			expect(c.activeTerminalId).toBeNull();
		});

		it('opens new sessions in a terminal when that is the default, and remembers the choice', async () => {
			const c = await connected();
			const calls = fakeServer();
			c.setDefaultView('terminal');
			await c.startClaude('/repo', undefined, 'repo');
			const create = calls.find((x) => x.method === 'POST' && x.path === '/remote/terminals');
			expect(create?.body).toMatchObject({
				claudeSession: { id: expect.stringMatching(/^[0-9a-f-]{36}$/) }
			});
			expect((create?.body as { command?: string }).command).toBeUndefined();
			expect(c.activeTerminalId).toBe('t1');
			expect(new MobileClient().defaultView).toBe('terminal');
		});

		it('opens a past session in the default view; the server decides it resumes', async () => {
			const c = await connected();
			const calls = fakeServer();
			const ref = { sessionId: SID, projectPath: '/repo', name: 'Old', claudeAccountId: 'work' };

			c.setDefaultView('terminal');
			await c.openClaude(ref);
			const create = calls.find((x) => x.method === 'POST' && x.path === '/remote/terminals');
			expect(create?.body).toMatchObject({
				claudeSession: { id: SID },
				claudeAccountId: 'work'
			});

			c.setDefaultView('chat');
			await c.openClaude(ref);
			expect(c.activeChat).toEqual(ref);
		});

		it('shows a desktop Claude session as chat or terminal without stopping or spawning it', async () => {
			const c = await connected();
			const calls = fakeServer({ withTerminal: true });
			const ref = c.chatRef(summary);
			c.openChat(ref);

			await c.showAsTerminal(ref);
			expect(c.activeChat).toBeNull();
			expect(c.activeTerminalId).toBe('t1');

			await c.showAsChat('t1');
			expect(c.activeChat).toEqual(ref);
			expect(c.terminals).toHaveLength(1);
			expect(c.terminalChats.t1).toEqual(ref);
			expect(calls.filter((x) => x.method !== 'GET')).toEqual([
				{
					method: 'POST',
					path: '/agent/claude',
					body: {
						projectPath: '/repo',
						worktreePath: '/repo-wt',
						sessionId: SID,
						claudeAccountId: 'work',
						attachOnly: true
					}
				}
			]);
		});

		it('keeps the chat open when its backing terminal is missing instead of launching another', async () => {
			const c = await connected();
			const calls = fakeServer();
			const ref = c.chatRef(summary);
			c.openChat(ref);
			await c.showAsTerminal(ref);
			expect(c.activeChat).toEqual(ref);
			expect(c.activeTerminalId).toBeNull();
			expect(c.notice).toMatch(/no running terminal/);
			expect(c.switching).toBe(false);
			expect(calls.every((x) => x.method === 'GET')).toBe(true);
		});

		it('counts a Claude session once, including when it is waiting for approval', async () => {
			const c = await connected();
			c.terminals = [
				{ id: 't1', cwd: '/repo', createdAt: 0, alive: true },
				{ id: 'shell', cwd: '/repo', createdAt: 0, alive: true }
			];
			c.chats = [{ ...summary, terminalId: 't1' }, codexSummary];
			expect(c.standaloneTerminals.map((t) => t.id)).toEqual(['shell']);
			c.chats[0].waiting = { id: 'a', tool: 'Bash', preview: 'ls', inTerminal: false };
			expect(c.standaloneTerminals.map((t) => t.id)).toEqual(['shell']);
			c.chats[0].exited = true;
			expect(c.standaloneTerminals.map((t) => t.id)).toEqual(['t1', 'shell']);
		});

		it('recognizes a Claude terminal before its plugin attaches without starting another process', async () => {
			const c = await connected();
			const terminal = {
				id: 'early',
				cwd: '/repo',
				createdAt: 0,
				alive: true,
				claudeSessionId: SID
			};
			const calls = routeFetch({
				'/agent': () => jsonResponse([]),
				'/remote/terminals': () => jsonResponse([terminal]),
				'/agent/claude': () => jsonResponse({ error: 'Not attached' }, 404)
			});
			await c.refreshTerminals();
			c.selectTerminal('early');
			expect(c.terminalChats.early.sessionId).toBe(SID);
			expect(c.standaloneTerminals).toHaveLength(1);
			await c.showAsChat('early');
			expect(c.activeTerminalId).toBe('early');
			expect(c.activeChat).toBeNull();
			expect(c.notice).toMatch(/login or trust prompt/);
			const post = calls.mock.calls.find(([, init]) => init?.method === 'POST');
			expect(JSON.parse(String(post?.[1]?.body)).attachOnly).toBe(true);
			expect(calls.mock.calls.some(([, init]) => init?.method === 'DELETE')).toBe(false);
		});

		it('follows /clear aliases to the same terminal and its current session id', async () => {
			const c = await connected();
			const current = {
				...summary,
				sessionId: 'after-clear',
				terminalId: 't1',
				previousIds: [SID]
			};
			routeFetch({
				'/agent': () => jsonResponse([current]),
				'/remote/terminals': () =>
					jsonResponse([{ id: 't1', cwd: '/repo-wt', createdAt: 0, alive: true }]),
				'/agent/claude': () => jsonResponse({ sessionId: 'after-clear', terminalId: 't1' })
			});
			c.openChat(c.chatRef(summary));
			await c.showAsTerminal(c.activeChat!);
			expect(c.activeTerminalId).toBe('t1');
			await c.showAsChat('t1');
			expect(c.activeChat).toMatchObject({
				sessionId: 'after-clear',
				attachOnly: true,
				claudeAccountId: 'work'
			});
		});

		it('does not reopen a chat after Back while its attach request is still pending', async () => {
			const c = await connected();
			fakeServer({ withTerminal: true });
			await c.refreshTerminals();
			c.selectTerminal('t1');
			let finish!: () => void;
			let attachStarted = false;
			const fetchServer = vi.mocked(fetch).getMockImplementation()!;
			vi.mocked(fetch).mockImplementation((input, init) => {
				if (init?.method === 'POST') {
					attachStarted = true;
					return new Promise((resolve) => {
						finish = () => resolve(jsonResponse({ sessionId: SID, terminalId: 't1' }));
					});
				}
				return fetchServer(input, init);
			});
			const opening = c.showAsChat('t1');
			await vi.waitFor(() => expect(attachStarted).toBe(true));
			c.closeTerminal();
			finish();
			await opening;
			expect(c.activeChat).toBeNull();
			expect(c.activeTerminalId).toBeNull();
			expect(c.switching).toBe(false);
		});

		it('answers an approval from the home screen', async () => {
			const c = await connected();
			const calls = fakeServer();
			await c.answer(SID, 'perm-1', 'allow');
			expect(calls[0]).toEqual({
				method: 'POST',
				path: `/agent/claude/${SID}/message`,
				body: { t: 'approve', requestId: 'perm-1', decision: 'allow' }
			});
		});

		it('keeps the terminal running when Claude has not attached to Chat yet', async () => {
			const c = await connected();
			const calls = fakeServer({ withTerminal: true, failAttach: true });
			const ref = c.chatRef(summary);
			c.openChat(ref);
			await c.showAsTerminal(ref);
			await c.showAsChat('t1');
			expect(c.activeChat).toBeNull();
			expect(c.activeTerminalId).toBe('t1');
			expect(c.notice).toMatch(/login or trust prompt/);
			expect(c.switching).toBe(false);
			expect(
				calls.some(
					(x) => x.method === 'DELETE' || (x.path === '/remote/terminals' && x.method === 'POST')
				)
			).toBe(false);
		});

		it('starts a Codex chat with no id, so the server picks the thread', async () => {
			const c = await connected();
			c.setDefaultView('terminal');
			c.startCodex('/repo', '/repo-wt', 'repo · wt');
			expect(c.activeChat).toEqual({
				sessionId: '',
				agent: 'codex',
				projectPath: '/repo',
				worktreePath: '/repo-wt',
				name: 'repo · wt'
			});
			expect(c.activeTerminalId).toBeNull();
		});

		it('opens a listed Codex session as a Codex chat', async () => {
			const c = await connected();
			expect(c.chatRef(codexSummary)).toEqual({
				sessionId: 'thread-1',
				attachOnly: true,
				agent: 'codex',
				projectPath: '/repo',
				worktreePath: undefined,
				name: 'Tidy the docs'
			});
			expect(c.chatRef(summary).agent).toBeUndefined();
		});

		it('names Codex when answering one of its approvals fails', async () => {
			const c = await connected();
			fakeServer();
			await c.refreshChats();
			vi.mocked(fetch).mockResolvedValueOnce(new Response(null, { status: 500 }));
			await c.answer('thread-1', 'perm-1', 'allow');
			expect(c.notice).toMatch(/^Couldn't answer Codex/);
		});

		it('ending a Codex chat before it has a thread just leaves its screen', async () => {
			const c = await connected();
			const calls = fakeServer();
			c.startCodex('/repo', undefined, 'repo');
			await c.endChat('');
			expect(c.activeChat).toBeNull();
			expect(calls.some((x) => x.method === 'DELETE')).toBe(false);
		});

		it('ending a chat leaves its screen even after /clear changed its id', async () => {
			const c = await connected();
			fakeServer();
			c.openChat(c.chatRef(summary));
			await c.endChat('a-newer-id-after-clear');
			expect(c.activeChat).toBeNull();
		});
	});

	it('restores the active machine on the next launch', async () => {
		expect(new MobileClient().hasSavedServer).toBe(false);
		await connected();
		const c = new MobileClient();
		expect(c.url).toBe('http://box:4317');
		expect(c.token).toBe(TOKEN);
		expect(c.hasSavedServer).toBe(true);
	});

	describe('scanAndConnect()', () => {
		const PAIR_URL = 'http://100.64.1.2:4317';

		const scanned = (content: string) => ({ content, format: 'QR_CODE', bounds: null });

		function scanner(overrides: Partial<Record<keyof QrScanner, unknown>> = {}) {
			const unregister = vi.fn(async () => {});
			let backHandler: (() => void) | undefined;
			const mock = {
				checkPermissions: vi.fn(async () => 'granted'),
				requestPermissions: vi.fn(async () => 'granted'),
				scan: vi.fn(async () => scanned(buildPairingUri({ url: PAIR_URL, token: TOKEN }))),
				cancel: vi.fn(async () => {}),
				onBackButtonPress: vi.fn(async (handler: () => void) => {
					backHandler = handler;
					return { unregister };
				}),
				...overrides
			} as unknown as QrScanner & Record<keyof QrScanner, ReturnType<typeof vi.fn>>;
			return { mock, unregister, pressBack: () => backHandler?.() };
		}

		/** A scan that stays pending until `resolve` is called, like the plugin. */
		function pendingScan() {
			let resolve!: (value: ReturnType<typeof scanned>) => void;
			const scan = vi.fn(() => new Promise((r) => (resolve = r)));
			return { scan, resolve: (content: string) => resolve(scanned(content)) };
		}

		it('scans a pairing code, fills in the server and connects', async () => {
			const fetchSpy = routeFetch({
				...CONNECT_ROUTES,
				'/remote/terminals': () => jsonResponse([])
			});
			const { mock: s } = scanner();
			const c = new MobileClient(s);

			await c.scanAndConnect();

			expect(s.scan).toHaveBeenCalledWith({ windowed: true, formats: ['QR_CODE'] });
			expect(c.connectError).toBeNull();
			expect(c.store).not.toBeNull();
			expect(c.url).toBe(PAIR_URL);
			expect(c.token).toBe(TOKEN);
			expect(c.machines.active?.token).toBe(TOKEN);
			expect(fetchSpy).toHaveBeenCalledWith(`${PAIR_URL}/remote/terminals`, expect.anything());
			expect(c.scanning).toBe(false);
		});

		it('requests camera permission when not yet granted', async () => {
			routeFetch({ ...CONNECT_ROUTES, '/remote/terminals': () => jsonResponse([]) });
			const { mock: s } = scanner({ checkPermissions: vi.fn(async () => 'prompt') });
			const c = new MobileClient(s);

			await c.scanAndConnect();

			expect(s.requestPermissions).toHaveBeenCalled();
			expect(c.store).not.toBeNull();
		});

		it('explains a denied camera permission without scanning', async () => {
			const { mock: s } = scanner({
				checkPermissions: vi.fn(async () => 'denied'),
				requestPermissions: vi.fn(async () => 'denied')
			});
			const c = new MobileClient(s);

			await c.scanAndConnect();

			expect(c.connectError).toBe(CAMERA_DENIED);
			expect(s.scan).not.toHaveBeenCalled();
		});

		it('rejects a QR code that is not a Workbench pairing code', async () => {
			const fetchSpy = routeFetch(CONNECT_ROUTES);
			const { mock: s } = scanner({
				scan: vi.fn(async () => ({
					content: 'https://example.com',
					format: 'QR_CODE',
					bounds: null
				}))
			});
			const c = new MobileClient(s);
			c.url = 'kept:4317';

			await c.scanAndConnect();

			expect(c.connectError).toBe(NOT_A_PAIRING_CODE);
			expect(c.url).toBe('kept:4317');
			expect(fetchSpy).not.toHaveBeenCalled();
		});

		it('stays silent when the scan is cancelled', async () => {
			const fetchSpy = routeFetch(CONNECT_ROUTES);
			const { mock: s } = scanner({ scan: vi.fn(async () => Promise.reject('cancelled')) });
			const c = new MobileClient(s);

			await c.scanAndConnect();

			expect(c.connectError).toBeNull();
			expect(c.store).toBeNull();
			expect(fetchSpy).not.toHaveBeenCalled();
			expect(c.scanning).toBe(false);
		});

		it('cancel ends the scan at once and ignores a late result', async () => {
			const fetchSpy = routeFetch(CONNECT_ROUTES);
			const pending = pendingScan();
			const { mock: s, unregister } = scanner({ scan: pending.scan });
			const c = new MobileClient(s);

			const scanning = c.scanAndConnect();
			await vi.waitFor(() => expect(s.scan).toHaveBeenCalled());
			expect(c.scanning).toBe(true);

			await c.cancelScan();
			expect(c.scanning).toBe(false);
			expect(s.cancel).toHaveBeenCalled();
			expect(unregister).toHaveBeenCalled();

			pending.resolve(buildPairingUri({ url: PAIR_URL, token: TOKEN }));
			await scanning;
			expect(c.url).toBe('');
			expect(c.connectError).toBeNull();
			expect(fetchSpy).not.toHaveBeenCalled();
		});

		it('the Android back button cancels the scan', async () => {
			const pending = pendingScan();
			const { mock: s, pressBack } = scanner({ scan: pending.scan });
			const c = new MobileClient(s);

			void c.scanAndConnect();
			await vi.waitFor(() => expect(s.scan).toHaveBeenCalled());
			pressBack();

			await vi.waitFor(() => expect(s.cancel).toHaveBeenCalled());
			expect(c.scanning).toBe(false);
		});

		it('a double tap starts only one scan', async () => {
			const pending = pendingScan();
			const { mock: s } = scanner({ scan: pending.scan });
			const c = new MobileClient(s);

			void c.scanAndConnect();
			void c.scanAndConnect();
			await vi.waitFor(() => expect(s.scan).toHaveBeenCalled());
			await c.scanAndConnect();

			expect(s.checkPermissions).toHaveBeenCalledTimes(1);
			expect(s.scan).toHaveBeenCalledTimes(1);
			await c.cancelScan();
		});

		it('a new scan after a cancel works', async () => {
			routeFetch({ ...CONNECT_ROUTES, '/remote/terminals': () => jsonResponse([]) });
			const pending = pendingScan();
			const { mock: s } = scanner({ scan: pending.scan });
			const c = new MobileClient(s);

			void c.scanAndConnect();
			await vi.waitFor(() => expect(s.scan).toHaveBeenCalledTimes(1));
			await c.cancelScan();

			s.scan.mockImplementationOnce(async () =>
				scanned(buildPairingUri({ url: PAIR_URL, token: TOKEN }))
			);
			await c.scanAndConnect();

			expect(c.store).not.toBeNull();
			expect(c.scanning).toBe(false);
		});

		it('connects to a scanned https origin exactly, without the default port', async () => {
			const origin = 'https://box.tail1234.ts.net';
			const fetchSpy = routeFetch({
				...CONNECT_ROUTES,
				'/remote/terminals': () => jsonResponse([])
			});
			const { mock: s } = scanner({
				scan: vi.fn(async () => scanned(buildPairingUri({ url: origin, token: TOKEN })))
			});
			const c = new MobileClient(s);

			await c.scanAndConnect();

			expect(c.url).toBe(origin);
			expect(fetchSpy).toHaveBeenCalledWith(`${origin}/health`, expect.anything());
			expect(c.machines.active?.url).toBe(origin);

			// Relaunch: the saved origin is reused as-is too.
			fetchSpy.mockClear();
			const relaunched = new MobileClient(s);
			await relaunched.connect();
			expect(fetchSpy).toHaveBeenCalledWith(`${origin}/health`, expect.anything());
		});

		it('surfaces other scanner errors', async () => {
			const { mock: s } = scanner({
				scan: vi.fn(async () => Promise.reject(new Error('no camera')))
			});
			const c = new MobileClient(s);

			await c.scanAndConnect();

			expect(c.connectError).toBe('no camera');
		});
	});
	it('keeps the current chat visible and reports a failed End session', async () => {
		const c = await connected({ '/agent/claude/s': () => jsonResponse({ error: 'offline' }, 503) });
		c.openChat({ sessionId: 's', projectPath: '/repo', name: 'repo' });
		await c.endChat('s');
		expect(c.activeChat?.sessionId).toBe('s');
		expect(c.notice).toMatch(/Couldn't end.*offline/);
	});

	it('keeps a terminal visible when deletion fails', async () => {
		const c = await connected({
			'/remote/terminals': () =>
				jsonResponse([{ id: 't1', cwd: '/repo', createdAt: 0, alive: true }]),
			'/remote/terminals/t1': () => jsonResponse({ error: 'offline' }, 503)
		});
		c.selectTerminal('t1');
		await c.killTerminal('t1');
		expect(c.activeTerminalId).toBe('t1');
		expect(c.notice).toMatch(/Couldn't close/);
	});

	it('loads accounts and carries the selected account into new chats', async () => {
		const c = await connected({
			'/settings/workbench': () =>
				jsonResponse({
					claudeAccounts: [{ id: 'work', name: 'Work', configDir: '/account' }],
					activeClaudeAccount: 'work'
				})
		});
		await c.startClaude('/repo', undefined, 'repo');
		expect(c.activeChat?.claudeAccountId).toBe('work');
		c.setAccount('');
		await c.startClaude('/repo', undefined, 'repo');
		expect(c.activeChat?.claudeAccountId).toBeUndefined();
	});

	it('updates a new Codex id without remounting its screen', async () => {
		const c = await connected();
		c.startCodex('/repo', undefined, 'repo');
		const key = c.chatScreenKey;
		c.updateChatId('thread-1');
		expect(c.activeChat?.sessionId).toBe('thread-1');
		expect(c.chatScreenKey).toBe(key);
	});
});
