import { describe, expect, it } from 'vitest';
import { terminalWsUrl, agentWsUrl } from './index.ts';

describe('agentWsUrl', () => {
	it('builds the chat session url with the token query', () => {
		expect(agentWsUrl('http://127.0.0.1:4317/', 'abc-123', 'se cr/et')).toBe(
			'ws://127.0.0.1:4317/agent/claude/abc-123/ws?token=se%20cr%2Fet'
		);
	});

	it('shares the scheme mapping with terminal urls', () => {
		expect(agentWsUrl('https://box', 'id')).toBe('wss://box/agent/claude/id/ws');
		expect(terminalWsUrl('https://box', 'id')).toBe('wss://box/remote/terminals/id/ws');
	});
});
