import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MobileClient } from './client.svelte.ts';
import { openExternal } from './open-external.ts';
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

	it("checks the host's update on connect and drops it on disconnect", async () => {
		const c = await connected({
			'/host/update': () =>
				jsonResponse({ current: '1.0.0', available: '1.1.0', installing: false })
		});
		await vi.waitFor(() => expect(c.hostUpdate?.status?.available).toBe('1.1.0'));
		c.disconnect();
		expect(c.hostUpdate).toBeNull();
	});

	it('connect() normalizes the url, sets the store, and loads the workspaces', async () => {
		const c = await connected({
			'/remote/terminals': () => jsonResponse([{ id: 't1', cwd: '/p', createdAt: 0, alive: true }])
		});
		expect(c.connectError).toBeNull();
		expect(c.store).not.toBeNull();
		expect(c.url).toBe('http://box:4317');
		expect(c.panes).toHaveLength(1);
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
});
