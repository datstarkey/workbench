import { createSubscriber } from 'svelte/reactivity';
import type { TranscriptMeta } from '@workbench/types';

/** Mirrors `workbench_core::claude_transcript::KEEPALIVE_PROMPT`; hook events for it aren't activity. */
export const KEEPALIVE_PROMPT = 'Workbench cache keep-alive. Reply with only "ok".';
/** Below this a compact saves too little to be worth offering (the server's floor too). */
export const MIN_COMPACT_TOKENS = 30_000;
/** Under this much time left (or a fifth of a shorter lifetime) the cache shows as about to expire. */
const LOW_MS = 5 * 60_000;
/** How far ahead the cache can be kept warm, in hours. */
export const KEEP_WARM_HOURS = [1, 2, 4, 8] as const;

export interface CacheState {
	expiresAt: number;
	ttlSecs: number;
	remainingMs: number;
	warm: boolean;
	/** Warm, with under 5 minutes left. */
	low: boolean;
	/** Of its lifetime, how much is left (0–100). */
	percent: number;
}

/** The prompt cache's state at `now`; null before any API call (or after a compact). */
export function cacheState(meta: TranscriptMeta | null, now: number): CacheState | null {
	const expiresAt = meta?.cacheExpiresAt;
	if (!expiresAt) return null;
	const ttlSecs = meta.cacheTtlSecs ?? 300;
	const remainingMs = Math.max(0, expiresAt - now);
	return {
		expiresAt,
		ttlSecs,
		remainingMs,
		warm: remainingMs > 0,
		low: remainingMs > 0 && remainingMs < Math.min(LOW_MS, (ttlSecs * 1000) / 5),
		percent: Math.min(100, (remainingMs / (ttlSecs * 1000)) * 100)
	};
}

/** `42m`, `1h 5m`, `<1m`. */
export function formatRemaining(ms: number): string {
	const minutes = Math.floor(ms / 60_000);
	if (minutes < 1) return '<1m';
	const hours = Math.floor(minutes / 60);
	return hours ? `${hours}h${minutes % 60 ? ` ${minutes % 60}m` : ''}` : `${minutes}m`;
}

/** The cache has expired on a conversation big enough that the next message re-sends a lot. */
export function offerCompact(meta: TranscriptMeta | null, now: number): boolean {
	const cache = cacheState(meta, now);
	return (
		cache !== null && !cache.warm && !meta?.busy && (meta?.contextTokens ?? 0) >= MIN_COMPACT_TOKENS
	);
}

const tick = createSubscriber((update) => {
	const timer = setInterval(update, 15_000);
	return () => clearInterval(timer);
});

/** `Date.now()` that re-runs the reading derived or template every 15s. */
export function cacheClock(): number {
	tick();
	return Date.now();
}
