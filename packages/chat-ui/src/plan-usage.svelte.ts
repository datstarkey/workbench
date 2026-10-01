import { IsDocumentVisible, onCleanup, watch, type IsDocumentVisibleOptions } from 'runed';
import type { RateLimitInfo, TranscriptMeta, UsageLimit } from '@workbench/types';
import { usageChips } from './usage-format.ts';

/** The server caches `/usage` for a minute, so this mostly bounds the requests. */
export const PLAN_USAGE_REFRESH_MS = 150_000;

export type LoadUsage = (fresh: boolean) => Promise<UsageLimit[]>;

/**
 * One account's plan limits on one server: checked on creation, every few
 * minutes while the window is visible, and on becoming visible again. Shared
 * by every chat showing them; get it with {@link usePlanUsage}.
 */
export class PlanUsage {
	limits = $state.raw<UsageLimit[]>([]);
	/** When the check behind `limits` started (ms). */
	#checkedAt = $state(0);
	#event = $state.raw<{ info: RateLimitInfo; at: number } | null>(null);
	/** A `rate_limit_event` is fresher than `/usage` only until a check starts after it. */
	readonly chips = $derived.by(() =>
		usageChips(
			this.limits,
			this.#event && this.#event.at > this.#checkedAt ? this.#event.info : null
		)
	);
	readonly #load: LoadUsage;
	#loading = false;
	#queued: { fresh: boolean } | null = null;

	constructor(load: LoadUsage, options?: IsDocumentVisibleOptions) {
		this.#load = load;
		const visible = new IsDocumentVisible(options);
		watch(
			() => visible.current,
			(shown) => {
				if (shown) void this.refresh();
			}
		);
		const timer = setInterval(() => {
			if (visible.current) void this.refresh();
		}, PLAN_USAGE_REFRESH_MS);
		onCleanup(() => clearInterval(timer));
	}

	showEvent(info: RateLimitInfo): void {
		this.#event = { info, at: Date.now() };
	}

	/**
	 * One check at a time: a request made during one runs once after it, fresh
	 * if any of them asked. A failed check keeps the last figures.
	 */
	async refresh(fresh = false): Promise<void> {
		if (this.#loading) {
			this.#queued = { fresh: fresh || (this.#queued?.fresh ?? false) };
			return;
		}
		this.#loading = true;
		const startedAt = Date.now();
		try {
			this.limits = await this.#load(fresh);
			this.#checkedAt = startedAt;
		} catch {
			// An older server without the route, or the CLI timed out.
		} finally {
			this.#loading = false;
		}
		const next = this.#queued;
		this.#queued = null;
		if (next) await this.refresh(next.fresh);
	}
}

/** Not reactive: only mounting and unmounting chats touch it. */
const shared: Record<string, { usage: PlanUsage; refs: number; dispose: () => void }> = {};

/**
 * The {@link PlanUsage} for `key` (server + account), shared by every mounted
 * chat with that key so they run one timer and show the same numbers. This
 * chat's rate-limit events show at once, and its turn ends (or a rate-limit
 * change while idle) ask for a fresh check. Call during component init.
 */
export function usePlanUsage(
	key: string,
	load: LoadUsage,
	meta: () => TranscriptMeta | null,
	options?: IsDocumentVisibleOptions
): PlanUsage {
	let entry = shared[key];
	if (!entry) {
		let usage!: PlanUsage;
		const dispose = $effect.root(() => {
			usage = new PlanUsage(load, options);
		});
		entry = { usage, refs: 0, dispose };
		shared[key] = entry;
	}
	const held = entry;
	held.refs++;
	watch(
		() => [JSON.stringify(meta()?.rateLimit ?? null), meta()?.busy ?? false] as const,
		([event, busy], previous) => {
			const info = meta()?.rateLimit;
			if (info && event !== previous?.[0]) held.usage.showEvent(info);
			if (!busy) void held.usage.refresh(true);
		},
		{ lazy: true }
	);
	onCleanup(() => {
		if (--held.refs > 0) return;
		held.dispose();
		delete shared[key];
	});
	return held.usage;
}
