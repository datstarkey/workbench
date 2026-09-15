import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MobileClient, normalizeUrl } from './client.svelte.ts';

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
});
