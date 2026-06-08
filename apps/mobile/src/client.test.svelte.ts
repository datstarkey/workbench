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
function routeFetch(routes: Record<string, () => Response | Promise<Response>>) {
	const spy = vi.fn((input: string) => {
		const path = new URL(input).pathname;
		const handler = routes[path];
		return Promise.resolve(handler ? handler() : jsonResponse(null));
	});
	vi.stubGlobal('fetch', spy);
	return spy;
}

const CONNECT_ROUTES = {
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

	async function connected(extra: Record<string, () => Response | Promise<Response>> = {}) {
		routeFetch({ ...CONNECT_ROUTES, '/remote/terminals': () => jsonResponse([]), ...extra });
		const c = new MobileClient();
		c.url = 'box:4317';
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

	it('connect() records an error and leaves the store null on a failed health check', async () => {
		routeFetch({ '/health': () => jsonResponse('no', 503) });
		const c = new MobileClient();
		c.url = 'box:4317';
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

	it('restores a previously saved server address from localStorage', () => {
		localStorage.setItem('wb.serverUrl', 'http://saved:4317');
		const c = new MobileClient();
		expect(c.url).toBe('http://saved:4317');
		expect(c.hasSavedServer).toBe(true);
	});
});
