import { IsDocumentVisible, onCleanup, watch, type IsDocumentVisibleOptions } from 'runed';
import type { TranscriptMeta, UsageLimit } from '@workbench/types';
import { usageChips } from './chat-format.ts';

/** The server caches `/usage` for a minute, so this mostly bounds the requests. */
export const PLAN_USAGE_REFRESH_MS = 150_000;

const clock = (secs: number) =>
	new Date(secs * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });

/**
 * The Claude plan's limits beside an open chat: checked on mount, every few
 * minutes while the window is visible, on becoming visible again, and after
 * each turn or rate-limit change (Claude reports a `rate_limit_event` per
 * message; an unchanged one sends no frame, so the turn ending stands in for
 * it). Construct during component init.
 */
export class PlanUsage {
	private readonly load: () => Promise<UsageLimit[]>;
	private readonly meta: () => TranscriptMeta | null;
	limits = $state.raw<UsageLimit[]>([]);
	readonly chips = $derived.by(() =>
		usageChips(this.limits, this.meta()?.rateLimit ?? null, clock)
	);
	#loading = false;

	constructor(
		load: () => Promise<UsageLimit[]>,
		meta: () => TranscriptMeta | null,
		options?: IsDocumentVisibleOptions
	) {
		this.load = load;
		this.meta = meta;
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
		watch(
			() => [JSON.stringify(this.meta()?.rateLimit ?? null), this.meta()?.busy ?? false] as const,
			([, busy]) => {
				if (!busy && visible.current) void this.refresh();
			},
			{ lazy: true }
		);
	}

	/** Keeps the last figures when a check fails. */
	async refresh(): Promise<void> {
		if (this.#loading) return;
		this.#loading = true;
		try {
			this.limits = await this.load();
		} catch {
			// An older server without the route, or the CLI timed out.
		} finally {
			this.#loading = false;
		}
	}
}
