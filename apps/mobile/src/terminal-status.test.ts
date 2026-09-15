import { describe, expect, it } from 'vitest';
import { statusForTextFrame, statusOnClose } from './terminal-status.ts';

describe('statusForTextFrame', () => {
	it('maps control frames to statuses', () => {
		expect(statusForTextFrame('{"t":"takeover"}')).toBe('taken_over');
		expect(statusForTextFrame('{"t":"exit","code":0}')).toBe('exited');
		expect(statusForTextFrame('{"t":"revoked"}')).toBe('closed');
	});

	it('returns null for anything else', () => {
		expect(statusForTextFrame('plain text')).toBeNull();
		expect(statusForTextFrame('{"t":"other"}')).toBeNull();
	});
});

describe('statusOnClose', () => {
	it('keeps a takeover or exit reason when the socket then closes', () => {
		expect(statusOnClose('taken_over')).toBe('taken_over');
		expect(statusOnClose('exited')).toBe('exited');
	});

	it('marks an unexpected drop as closed', () => {
		expect(statusOnClose('open')).toBe('closed');
		expect(statusOnClose('connecting')).toBe('closed');
	});
});
