import { describe, expect, it } from 'vitest';
import { reconnectDelay, statusForTextFrame, statusOnClose } from './terminal-status.ts';

describe('statusForTextFrame', () => {
	it('maps control frames to statuses', () => {
		expect(statusForTextFrame('{"t":"takeover"}')).toBe('taken_over');
		expect(statusForTextFrame('{"t":"exit","code":0}')).toBe('exited');
		expect(statusForTextFrame('{"t":"revoked"}')).toBe('revoked');
	});

	it('returns null for anything else', () => {
		expect(statusForTextFrame('plain text')).toBeNull();
		expect(statusForTextFrame('{"t":"other"}')).toBeNull();
	});
});

describe('statusOnClose', () => {
	it('keeps a takeover, exit or revoked reason when the socket then closes', () => {
		expect(statusOnClose('taken_over')).toBe('taken_over');
		expect(statusOnClose('exited')).toBe('exited');
		expect(statusOnClose('revoked')).toBe('revoked');
	});

	it('treats an unexpected drop as one to reconnect', () => {
		expect(statusOnClose('open')).toBe('reconnecting');
		expect(statusOnClose('connecting')).toBe('reconnecting');
	});
});

describe('reconnectDelay', () => {
	it('backs off and caps', () => {
		expect([0, 1, 2, 10].map(reconnectDelay)).toEqual([500, 1000, 2000, 10_000]);
	});
});
