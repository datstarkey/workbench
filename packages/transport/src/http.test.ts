import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
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

	it('maps Git review to the registered project and exact worktree/file', async () => {
		const f = mockFetch(() => json('diff'));
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		await t.invoke('git_status', { path: '/repo' });
		expect(f.mock.calls[0][0]).toBe('http://host:4317/projects/git-status?projectPath=%2Frepo');
		await t.invoke('git_file_diff', {
			path: '/repo-wt',
			projectPath: '/repo',
			file: 'space & name.ts',
			staged: false
		});
		const url = new URL(f.mock.calls[1][0]);
		expect(url.pathname).toBe('/projects/git-diff');
		expect(Object.fromEntries(url.searchParams)).toEqual({
			projectPath: '/repo',
			worktreePath: '/repo-wt',
			file: 'space & name.ts',
			staged: 'false'
		});
	});

	it("puts the host's active Claude account, null for the default login", async () => {
		const f = mockFetch(() => new Response(null, { status: 204 }));
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		await t.invoke('set_active_claude_account', { id: null });
		const [url, init] = f.mock.calls[0];
		expect(url).toBe('http://host:4317/settings/active-claude-account');
		expect(init.method).toBe('PUT');
		expect(JSON.parse(String(init.body))).toEqual({ id: null });
	});

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

	it('maps github_get_remote to GET /projects/github-remote and passes null through', async () => {
		const f = mockFetch(() => json(null));
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		const res = await t.invoke('github_get_remote', { path: '/repo' });
		expect(f.mock.calls[0][0]).toBe('http://host:4317/projects/github-remote?path=%2Frepo');
		expect(res).toBeNull();
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

	it('maps the host update to GET/POST /host/update and keeps the HTTP status on errors', async () => {
		const f = mockFetch((_url, init) =>
			init.method === 'POST'
				? json({ version: '1.1.0' }, 202)
				: json({ error: "this server can't update its host" }, 501)
		);
		const t = createHttpTransport({ baseUrl: 'http://host:4317', token: 'secret' });
		await expect(t.invoke('host_update_status', undefined)).rejects.toMatchObject({ status: 501 });
		await expect(t.invoke('host_update_status', { fresh: true })).rejects.toMatchObject({
			status: 501
		});
		expect(await t.invoke('host_update_install', { version: '1.1.0' })).toEqual({
			version: '1.1.0'
		});
		expect(f.mock.calls.map(([url, init]) => [init.method, url, init.body])).toEqual([
			['GET', 'http://host:4317/host/update', undefined],
			['GET', 'http://host:4317/host/update?fresh=true', undefined],
			['POST', 'http://host:4317/host/update', JSON.stringify({ version: '1.1.0' })]
		]);
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
		const unsub = await t.subscribe('codex:notify', (p) => seen.push(p));
		t.emitMockEvent('codex:notify', { a: 1 });
		unsub();
		t.emitMockEvent('codex:notify', { a: 2 });
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
		/** Simulate a successful connection (resets the backoff). */
		open() {
			this.handlers.open?.forEach((cb) => cb());
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
		await t.subscribe('codex:notify', () => {});
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

	it('resets the backoff to the base delay after a successful open', async () => {
		vi.useFakeTimers();
		vi.stubGlobal('WebSocket', FakeWS);

		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		await t.subscribe('codex:notify', () => {});

		// Fail twice so the next scheduled delay has backed off (2s→4s→8s).
		FakeWS.instances[0].fail();
		await vi.advanceTimersByTimeAsync(2000);
		FakeWS.instances[1].fail();
		await vi.advanceTimersByTimeAsync(4000);
		expect(FakeWS.instances).toHaveLength(3);

		// Third socket connects successfully, then drops → backoff must be back at the
		// 2s base, so the reconnect fires after 2s (not the backed-off ~16s).
		FakeWS.instances[2].open();
		FakeWS.instances[2].fail();
		await vi.advanceTimersByTimeAsync(2000);
		expect(FakeWS.instances).toHaveLength(4);
	});

	it('does not open a socket or reconnect when there are no subscribers', async () => {
		vi.useFakeTimers();
		vi.stubGlobal('WebSocket', FakeWS);
		createHttpTransport({ baseUrl: 'http://host:4317' });
		await vi.advanceTimersByTimeAsync(60000);
		expect(FakeWS.instances).toHaveLength(0);
	});
});

describe('HttpTransport request timeouts', () => {
	beforeEach(() => {
		vi.useFakeTimers();
		// A server that never answers: the fetch rejects only when its signal fires.
		mockFetch(
			(_url, init) =>
				new Promise((_, reject) =>
					init.signal?.addEventListener('abort', () =>
						reject(new DOMException('aborted', 'AbortError'))
					)
				)
		);
	});
	afterEach(() => {
		vi.useRealTimers();
		vi.unstubAllGlobals();
	});

	/** Milliseconds until `invoke` gives up, and the error it gives up with. */
	async function budget(run: () => Promise<unknown>) {
		let error: Error | null = null;
		void run().catch((e: Error) => (error = e));
		let waited = 0;
		while (!error && waited < 300_000) {
			await vi.advanceTimersByTimeAsync(1000);
			waited += 1000;
		}
		return { waited, message: (error as Error | null)?.message };
	}

	it('rejects a stalled request with a clear timeout error', async () => {
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		await expect(budget(() => t.invoke('list_projects', undefined))).resolves.toEqual({
			waited: 10_000,
			message: 'workbench-server: GET /projects timed out after 10s: the server is not responding'
		});
	});

	it('keeps other failures as they are', async () => {
		mockFetch(() => Promise.reject(new TypeError('Failed to fetch')));
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		await expect(t.invoke('list_projects', undefined)).rejects.toThrow('Failed to fetch');
	});

	it('gives routes that do slow work server-side a longer budget', async () => {
		const t = createHttpTransport({ baseUrl: 'http://host:4317' });
		const create = await budget(() => t.invoke('create_worktree', { request: {} } as never));
		expect(create.waited).toBe(120_000);
		const status = await budget(() => t.invoke('git_status', { path: '/repo' }));
		expect(status.waited).toBe(30_000);
	});
});
