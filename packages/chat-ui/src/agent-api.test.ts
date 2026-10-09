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

	it("builds the session's socket URL without a lookup", async () => {
		const fetch = stubFetch(null);
		await expect(api.socketUrl('a b')).resolves.toBe(
			'ws://box/agent/claude/a%20b/ws?token=t&meta=changed'
		);
		expect(fetch).not.toHaveBeenCalled();
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
		await expect(budget(() => api.taskTranscript('sid', 't1'))).resolves.toEqual({
			waited: 10_000,
			message:
				'GET /agent/claude/sid/tasks/t1/transcript timed out after 10s: the server is not responding'
		});
	});
});
