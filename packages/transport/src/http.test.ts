import { afterEach, describe, expect, it, vi } from 'vitest';
import { createHttpTransport } from './http.ts';
import { createMockTransport } from './mock.ts';

function mockFetch(impl: (url: string, init: RequestInit) => Response | Promise<Response>) {
	const spy = vi.fn(impl);
	vi.stubGlobal('fetch', spy);
	return spy;
}

function json(body: unknown, status = 200) {
	return new Response(JSON.stringify(body), {
		status,
		headers: { 'content-type': 'application/json' }
	});
}

describe('HttpTransport route mapping', () => {
	afterEach(() => vi.unstubAllGlobals());

	it('maps list_projects to GET /projects', async () => {
		const f = mockFetch(() => json([{ name: 'a', path: '/a' }]));
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		const res = await t.invoke('list_projects', undefined);
		expect(f).toHaveBeenCalledOnce();
		const [url, init] = f.mock.calls[0];
		expect(url).toBe('http://host:4317/projects');
		expect(init.method).toBe('GET');
		expect(res).toEqual([{ name: 'a', path: '/a' }]);
	});

	it('puts path in the query string for list_worktrees', async () => {
		const f = mockFetch(() => json([]));
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		await t.invoke('list_worktrees', { path: '/repo' });
		expect(f.mock.calls[0][0]).toBe('http://host:4317/projects/worktrees?path=%2Frepo');
	});

	it('drops undefined query params instead of serializing "undefined"', async () => {
		const f = mockFetch(() => json(null));
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		// path intentionally missing
		await t.invoke('list_worktrees', {} as never);
		expect(f.mock.calls[0][0]).toBe('http://host:4317/projects/worktrees');
	});

	it('create_worktree returns the bare string result', async () => {
		mockFetch(() => json('/repo-feature'));
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		const path = await t.invoke('create_worktree', { request: { repoPath: '/repo' } });
		expect(path).toBe('/repo-feature');
	});

	it('remote_spawn POSTs the body', async () => {
		const f = mockFetch(() => json({ id: 'x', status: 'starting' }));
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		await t.invoke('remote_spawn', { projectPath: '/p', name: 'n' });
		const [url, init] = f.mock.calls[0];
		expect(url).toBe('http://host:4317/remote/spawn');
		expect(init.method).toBe('POST');
		expect(JSON.parse(init.body as string)).toEqual({ projectPath: '/p', name: 'n' });
	});

	it('remote_kill maps to DELETE with the id in the path', async () => {
		const f = mockFetch(() => new Response(null, { status: 204 }));
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		const res = await t.invoke('remote_kill', { id: 'abc 1' });
		expect(f.mock.calls[0][0]).toBe('http://host:4317/remote/sessions/abc%201');
		expect(f.mock.calls[0][1].method).toBe('DELETE');
		expect(res).toBeUndefined();
	});

	it('sends the bearer token when configured', async () => {
		const f = mockFetch(() => json([]));
		const t = createHttpTransport({ baseUrl: 'http://host:4317', token: 'secret' });
		await t.invoke('list_projects', undefined);
		const headers = f.mock.calls[0][1].headers as Record<string, string>;
		expect(headers.authorization).toBe('Bearer secret');
	});

	it('throws with the server error message on non-ok responses', async () => {
		mockFetch(() => json({ error: 'boom' }, 500));
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		await expect(t.invoke('list_projects', undefined)).rejects.toThrow(/boom/);
	});

	it('throws for commands the server does not expose', async () => {
		mockFetch(() => json(null));
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		await expect(t.invoke('save_projects', { projects: [] })).rejects.toThrow(/not supported/);
	});
});

describe('MockTransport', () => {
	it('routes invoke to the registered handler', async () => {
		const t = createMockTransport();
		t.mockInvoke('list_projects', () => [{ name: 'm', path: '/m' }]);
		expect(await t.invoke('list_projects', undefined)).toEqual([{ name: 'm', path: '/m' }]);
	});

	it('delivers emitted events to subscribers and stops after unsubscribe', async () => {
		const t = createMockTransport();
		const seen: unknown[] = [];
		const unsub = await t.subscribe('claude:hook', (p) => seen.push(p));
		t.emitMockEvent('claude:hook', { a: 1 });
		unsub();
		t.emitMockEvent('claude:hook', { a: 2 });
		expect(seen).toEqual([{ a: 1 }]);
	});
});

describe('HttpTransport event socket reconnect backoff', () => {
	// Minimal WebSocket stand-in: records instances and lets the test drive the
	// error/close handlers without a real server (the server has no /events yet).
	class FakeWS {
		static instances: FakeWS[] = [];
		private handlers: Record<string, Array<(ev?: unknown) => void>> = {};
		constructor(public url: string) {
			FakeWS.instances.push(this);
		}
		addEventListener(type: string, cb: (ev?: unknown) => void) {
			(this.handlers[type] ??= []).push(cb);
		}
		close() {
			this.handlers.close?.forEach((cb) => cb());
		}
		/** Simulate a failed connection (error → close → reconnect scheduled). */
		fail() {
			this.handlers.error?.forEach((cb) => cb());
		}
	}

	afterEach(() => {
		vi.useRealTimers();
		vi.unstubAllGlobals();
		FakeWS.instances = [];
	});

	it('backs off exponentially instead of reconnecting every 2s', async () => {
		vi.useFakeTimers();
		vi.stubGlobal('WebSocket', FakeWS);

		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		await t.subscribe('claude:hook', () => {});
		expect(FakeWS.instances).toHaveLength(1);
		expect(FakeWS.instances[0].url).toBe('ws://host:4317/events');

		// First failure → reconnect scheduled at the 2s base delay.
		FakeWS.instances[0].fail();
		await vi.advanceTimersByTimeAsync(2000);
		expect(FakeWS.instances).toHaveLength(2);

		// Second failure → next attempt is backed off to 4s, not another 2s.
		FakeWS.instances[1].fail();
		await vi.advanceTimersByTimeAsync(2000);
		expect(FakeWS.instances).toHaveLength(2); // still waiting (only 2s of 4s elapsed)
		await vi.advanceTimersByTimeAsync(2000);
		expect(FakeWS.instances).toHaveLength(3); // fires at 4s
	});

	it('does not open a socket or reconnect when there are no subscribers', async () => {
		vi.useFakeTimers();
		vi.stubGlobal('WebSocket', FakeWS);
		createHttpTransport({ baseUrl: 'http://host:4317' });
		await vi.advanceTimersByTimeAsync(60000);
		expect(FakeWS.instances).toHaveLength(0);
	});
});
