import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { agentClient } from './agent-api';

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

	it('attaches to a running session by its id or one a /clear replaced', async () => {
		const fetch = stubFetch([
			{ sessionId: 'other' },
			{ sessionId: 'new-id', previousIds: ['sid'] }
		]);
		await expect(api.attach('new-id')).resolves.toBe('new-id');
		await expect(api.attach('sid')).resolves.toBe('new-id');
		expect(fetch.mock.calls.map((c) => c[0])).toEqual(['http://box/agent', 'http://box/agent']);
		await expect(api.attach('gone')).rejects.toMatchObject({ status: 404 });
	});

	it('reads a subagent transcript, null before it exists', async () => {
		const fetch = stubFetch(null);
		await expect(api.taskTranscript('sid', 'toolu_1/x')).resolves.toBeNull();
		expect(fetch.mock.calls[0][0]).toBe('http://box/agent/claude/sid/tasks/toolu_1%2Fx/transcript');
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
		await expect(budget(() => api.attach('sid'))).resolves.toEqual({
			waited: 10_000,
			message: 'GET /agent timed out after 10s: the server is not responding'
		});
	});
});
