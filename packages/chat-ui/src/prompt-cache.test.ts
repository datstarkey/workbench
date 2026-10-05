import { describe, expect, it } from 'vitest';
import type { TranscriptMeta } from '@workbench/types';
import { cacheState, formatRemaining, offerCompact } from './prompt-cache';

const meta = (over: Partial<TranscriptMeta>): TranscriptMeta => ({
	title: null,
	model: null,
	permissionMode: null,
	contextTokens: 80_000,
	busy: false,
	tasks: [],
	retry: null,
	rateLimit: null,
	models: [],
	modelChoice: null,
	effort: null,
	...over
});

describe('prompt cache', () => {
	it('counts down the cache lifetime', () => {
		const m = meta({ cacheExpiresAt: 3_600_000, cacheTtlSecs: 3600 });
		expect(cacheState(m, 0)).toMatchObject({ warm: true, low: false, percent: 100 });
		expect(cacheState(m, 3_400_000)).toMatchObject({ warm: true, low: true });
		expect(cacheState(m, 3_600_000)).toMatchObject({ warm: false, remainingMs: 0, percent: 0 });
		expect(cacheState(meta({}), 0)).toBeNull();
	});

	it('formats the time left', () => {
		expect(formatRemaining(30_000)).toBe('<1m');
		expect(formatRemaining(42 * 60_000)).toBe('42m');
		expect(formatRemaining(60 * 60_000)).toBe('1h');
		expect(formatRemaining(65 * 60_000)).toBe('1h 5m');
	});

	it('offers a compact only for a cold cache on a large, idle conversation', () => {
		const cold = meta({ cacheExpiresAt: 1000 });
		expect(offerCompact(cold, 2000)).toBe(true);
		expect(offerCompact(cold, 500)).toBe(false);
		expect(offerCompact({ ...cold, contextTokens: 5000 }, 2000)).toBe(false);
		expect(offerCompact({ ...cold, busy: true }, 2000)).toBe(false);
	});
});
