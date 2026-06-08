import { describe, expect, it } from 'vitest';
import { terminalWsUrl } from './terminal-url.ts';

describe('terminalWsUrl', () => {
	it('maps http→ws and omits the token query when none is set', () => {
		expect(terminalWsUrl('http://box:4317', 'abc')).toBe('ws://box:4317/remote/terminals/abc/ws');
	});

	it('maps https→wss', () => {
		expect(terminalWsUrl('https://box:4317', 'abc')).toBe('wss://box:4317/remote/terminals/abc/ws');
	});

	it('appends the bearer token as a url-encoded ?token= query', () => {
		expect(terminalWsUrl('http://box:4317', 'abc', 'se cr/et')).toBe(
			'ws://box:4317/remote/terminals/abc/ws?token=se%20cr%2Fet'
		);
	});

	it('strips a trailing slash from the server url', () => {
		expect(terminalWsUrl('http://box:4317/', 'abc')).toBe('ws://box:4317/remote/terminals/abc/ws');
	});

	it('does not append a token query for an empty token', () => {
		expect(terminalWsUrl('http://box:4317', 'abc', '')).toBe(
			'ws://box:4317/remote/terminals/abc/ws'
		);
	});
});
