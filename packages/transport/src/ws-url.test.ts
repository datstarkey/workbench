import { describe, expect, it } from 'vitest';
import { terminalWsUrl, agentWsUrl } from './index.ts';

describe('agentWsUrl', () => {
	it('builds the chat session url with the token query', () => {
		expect(agentWsUrl('http://127.0.0.1:4317/', 'abc-123', 'se cr/et')).toBe(
			'ws://127.0.0.1:4317/agent/claude/abc-123/ws?token=se%20cr%2Fet&meta=changed'
		);
	});

	it('shares the scheme mapping with terminal urls', () => {
		expect(agentWsUrl('https://box', 'id')).toBe('wss://box/agent/claude/id/ws?meta=changed');
		expect(terminalWsUrl('https://box', 'id')).toBe('wss://box/remote/terminals/id/ws');
	});
});

describe('terminalWsUrl', () => {
	it('url-encodes the token and leaves it out when empty', () => {
		expect(terminalWsUrl('http://box:4317', 'abc', 'se cr/et')).toBe(
			'ws://box:4317/remote/terminals/abc/ws?token=se%20cr%2Fet'
		);
		expect(terminalWsUrl('http://box:4317', 'abc', '')).toBe(
			'ws://box:4317/remote/terminals/abc/ws'
		);
	});

	it('strips a trailing slash from the server url', () => {
		expect(terminalWsUrl('http://box:4317/', 'abc')).toBe('ws://box:4317/remote/terminals/abc/ws');
	});
});
