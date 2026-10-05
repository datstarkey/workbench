import { afterEach, describe, expect, it, vi } from 'vitest';
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
