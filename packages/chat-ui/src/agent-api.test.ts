import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { agentClient, NeedsTrustError } from './agent-api';

function stubFetch(body: unknown) {
	const fetch = vi.fn(
		async (_url: string, _init?: RequestInit) => new Response(JSON.stringify(body), { status: 200 })
	);
	vi.stubGlobal('fetch', fetch);
	return fetch;
}

describe('agentClient', () => {
	afterEach(() => vi.unstubAllGlobals());
	const api = agentClient(() => ({ baseUrl: 'http://box', token: 't' }));

	it('starts on the agent’s route and returns the session id', async () => {
		const fetch = stubFetch({ sessionId: 'thread-1' });
		await expect(api.start({ agent: 'codex', projectPath: '/repo' })).resolves.toBe('thread-1');
		expect(fetch.mock.calls[0][0]).toBe('http://box/agent/codex');
		expect(JSON.parse(String(fetch.mock.calls[0][1]?.body))).toEqual({
			agent: 'codex',
			projectPath: '/repo'
		});

		await api.start({ projectPath: '/repo', sessionId: 'sid' });
		expect(fetch.mock.calls[1][0]).toBe('http://box/agent/claude');
	});

	it('rejects with the folder when Claude Code asks to trust it', async () => {
		stubFetch({ needsTrust: '/repo-feat' });
		const start = api.start({ projectPath: '/repo', sessionId: 'sid' });
		await expect(start).rejects.toBeInstanceOf(NeedsTrustError);
		await expect(start).rejects.toMatchObject({ path: '/repo-feat' });
	});

	it('lists sessions of every agent', async () => {
		const fetch = stubFetch([]);
		await api.list();
		expect(fetch.mock.calls[0][0]).toBe('http://box/agent');
	});

	it('asks other devices to close the chat only for an End', async () => {
		const fetch = stubFetch(null);
		await api.stop('sid');
		await api.stop('sid', { end: true });
		expect(fetch.mock.calls.map((c) => c[0])).toEqual([
			'http://box/agent/claude/sid',
			'http://box/agent/claude/sid?end=true'
		]);
	});

	it('ends a closed pane’s chat only when asked, and says when a joined chat was ended', async () => {
		const fetch = stubFetch(null);
		await api.stopPane('p 1');
		await api.stopPane('p 1', { end: true });
		expect(fetch.mock.calls.map((c) => c[0])).toEqual([
			'http://box/agent/claude?paneId=p%201',
			'http://box/agent/claude?paneId=p%201&end=true'
		]);

		vi.stubGlobal(
			'fetch',
			vi.fn(
				async () => new Response(JSON.stringify({ error: 'gone', ended: true }), { status: 404 })
			)
		);
		await expect(
			api.start({ projectPath: '/repo', sessionId: 'sid', attachOnly: true })
		).rejects.toMatchObject({ status: 404, ended: true, message: 'gone' });
	});

	it('reads a subagent transcript, null before it exists', async () => {
		const fetch = stubFetch(null);
		await expect(api.taskTranscript('sid', 'toolu_1/x')).resolves.toBeNull();
		expect(fetch.mock.calls[0][0]).toBe('http://box/agent/claude/sid/tasks/toolu_1%2Fx/transcript');
	});

	it('falls back to the Claude list on a server older than Codex chat', async () => {
		const fetch = vi.fn(async (url: string) =>
			url.endsWith('/agent')
				? new Response('{}', { status: 404 })
				: new Response(JSON.stringify([{ sessionId: 's1' }]), { status: 200 })
		);
		vi.stubGlobal('fetch', fetch);
		await expect(api.list()).resolves.toEqual([{ sessionId: 's1', agent: 'claude' }]);
		expect(fetch.mock.calls[1][0]).toBe('http://box/agent/claude');
	});
});

describe('agentClient timeouts', () => {
	beforeEach(() => {
		vi.useFakeTimers();
		// A server that never answers: the fetch rejects only when its signal fires.
		vi.stubGlobal(
			'fetch',
			vi.fn(
				(_url: string, init?: RequestInit) =>
					new Promise((_, reject) =>
						init?.signal?.addEventListener('abort', () =>
							reject(new DOMException('aborted', 'AbortError'))
						)
					)
			)
		);
	});
	afterEach(() => {
		vi.useRealTimers();
		vi.unstubAllGlobals();
	});
	const api = agentClient(() => ({ baseUrl: 'http://box', token: 't' }));

	/** Milliseconds until a request gives up, and the error it gives up with. */
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
		await expect(budget(() => api.list())).resolves.toEqual({
			waited: 10_000,
			message: 'GET /agent timed out after 10s: the server is not responding'
		});
	});

	it('waits longer for a start, which waits for the session to come up', async () => {
		const start = await budget(() => api.start({ projectPath: '/repo', sessionId: 'sid' }));
		expect(start.waited).toBe(90_000);
	});
});
