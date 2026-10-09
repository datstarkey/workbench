import { vi } from 'vitest';

/** Object-backed localStorage stub (jsdom's may lack `clear`). */
export function stubLocalStorage() {
	const mem: Record<string, string> = {};
	vi.stubGlobal('localStorage', {
		get length() {
			return Object.keys(mem).length;
		},
		key: (i: number) => Object.keys(mem)[i] ?? null,
		getItem: (k: string) => (k in mem ? mem[k] : null),
		setItem: (k: string, v: string) => void (mem[k] = String(v)),
		removeItem: (k: string) => void delete mem[k],
		clear: () => {
			for (const k of Object.keys(mem)) delete mem[k];
		}
	});
}

export function jsonResponse(body: unknown, status = 200) {
	return new Response(JSON.stringify(body), {
		status,
		headers: { 'content-type': 'application/json' }
	});
}

/** Stub fetch, routing by URL pathname; unknown paths return null JSON 200. */
export type Route = (init?: RequestInit, url?: URL) => Response | Promise<Response>;

export function routeFetch(routes: Record<string, Route>) {
	const spy = vi.fn((input: string, init?: RequestInit) => {
		const url = new URL(input);
		const handler = routes[url.pathname];
		return Promise.resolve(handler ? handler(init, url) : jsonResponse(null));
	});
	vi.stubGlobal('fetch', spy);
	return spy;
}

export const TOKEN = 'mobile-token-0123456789abcdef012345';

export const CONNECT_ROUTES: Record<string, Route> = {
	'/health': () => jsonResponse('ok'),
	'/projects': () => jsonResponse([])
};

/** An EventSource the test drives: `emit` delivers an event, `close` is recorded. */
export class FakeEventSource extends EventTarget {
	closed = false;
	constructor(readonly url: string) {
		super();
	}
	close() {
		this.closed = true;
	}
	emit(type: string, data?: unknown) {
		this.dispatchEvent(
			data === undefined ? new Event(type) : new MessageEvent(type, { data: JSON.stringify(data) })
		);
	}
}
