import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
	CAMERA_DENIED,
	MobileClient,
	NOT_A_PAIRING_CODE,
	normalizeUrl,
	type QrScanner
} from './client.svelte.ts';
import { buildPairingUri } from '@workbench/transport';

/** Object-backed localStorage stub (jsdom's may lack `clear`). */
function stubLocalStorage() {
	const mem: Record<string, string> = {};
	vi.stubGlobal('localStorage', {
		getItem: (k: string) => (k in mem ? mem[k] : null),
		setItem: (k: string, v: string) => void (mem[k] = String(v)),
		removeItem: (k: string) => void delete mem[k],
		clear: () => {
			for (const k of Object.keys(mem)) delete mem[k];
		}
	});
}

function jsonResponse(body: unknown, status = 200) {
	return new Response(JSON.stringify(body), {
		status,
		headers: { 'content-type': 'application/json' }
	});
}

/** Stub fetch, routing by URL pathname; unknown paths return null JSON 200. */
type Route = (init?: RequestInit) => Response | Promise<Response>;

function routeFetch(routes: Record<string, Route>) {
	const spy = vi.fn((input: string, init?: RequestInit) => {
		const path = new URL(input).pathname;
		const handler = routes[path];
		return Promise.resolve(handler ? handler(init) : jsonResponse(null));
	});
	vi.stubGlobal('fetch', spy);
	return spy;
}

const TOKEN = 'mobile-token-0123456789abcdef012345';

const CONNECT_ROUTES: Record<string, Route> = {
	'/health': () => jsonResponse('ok'),
	'/projects': () => jsonResponse([]),
	'/remote/sessions': () => jsonResponse([])
};

describe('normalizeUrl', () => {
	it('adds scheme and default port to a bare host', () => {
		expect(normalizeUrl('100.1.2.3')).toBe('http://100.1.2.3:4317');
	});
	it('keeps an explicit scheme/port and strips a trailing slash', () => {
		expect(normalizeUrl('https://box:9000/')).toBe('https://box:9000');
	});
	it('returns empty for blank input', () => {
		expect(normalizeUrl('   ')).toBe('');
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
		expect(localStorage.getItem('wb.serverUrl')).toBeNull();
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
		expect(localStorage.getItem('wb.token')).toBeNull();
	});

	it('connect() saves the url and token and sends the token on requests', async () => {
		const c = await connected({
			'/remote/terminals': (init) => {
				expect((init?.headers as Record<string, string>)?.authorization).toBe(`Bearer ${TOKEN}`);
				return jsonResponse([]);
			}
		});
		expect(c.connectError).toBeNull();
		expect(localStorage.getItem('wb.serverUrl')).toBe('http://box:4317');
		expect(localStorage.getItem('wb.token')).toBe(TOKEN);
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

	it('restores a previously saved server address and token from localStorage', () => {
		localStorage.setItem('wb.serverUrl', 'http://saved:4317');
		expect(new MobileClient().hasSavedServer).toBe(false);
		localStorage.setItem('wb.token', TOKEN);
		const c = new MobileClient();
		expect(c.url).toBe('http://saved:4317');
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
			expect(localStorage.getItem('wb.token')).toBe(TOKEN);
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
			expect(fetchSpy).toHaveBeenCalledWith(`${origin}/health`);
			expect(localStorage.getItem('wb.serverUrl')).toBe(origin);

			// Relaunch: the saved origin is reused as-is too.
			fetchSpy.mockClear();
			const relaunched = new MobileClient(s);
			await relaunched.connect();
			expect(fetchSpy).toHaveBeenCalledWith(`${origin}/health`);
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
});
