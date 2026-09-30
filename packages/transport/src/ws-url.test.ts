import { describe, expect, it } from 'vitest';
import { terminalWsUrl, transcriptWsUrl } from './index.ts';

describe('transcriptWsUrl', () => {
	it('builds the transcript stream url with the token query', () => {
		expect(transcriptWsUrl('http://127.0.0.1:4317/', 'abc-123', 'se cr/et')).toBe(
			'ws://127.0.0.1:4317/claude/transcripts/abc-123/ws?token=se%20cr%2Fet'
		);
	});

	it('shares the scheme mapping with terminal urls', () => {
		expect(transcriptWsUrl('https://box', 'id')).toBe('wss://box/claude/transcripts/id/ws');
		expect(terminalWsUrl('https://box', 'id')).toBe('wss://box/remote/terminals/id/ws');
	});
});
