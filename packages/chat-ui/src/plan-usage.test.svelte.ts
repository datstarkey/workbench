// @vitest-environment jsdom
import { flushSync } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { TranscriptMeta, UsageLimit } from '@workbench/types';
import { PLAN_USAGE_REFRESH_MS, PlanUsage } from './plan-usage.svelte';

const limits: UsageLimit[] = [{ label: 'session', percent: 42 }];

function fakeDocument() {
	const target = new EventTarget();
	return Object.assign(target, { hidden: false }) as unknown as Document & { hidden: boolean };
}

describe('PlanUsage', () => {
	let cleanup: () => void;
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => {
		cleanup?.();
		vi.useRealTimers();
	});

	function setup() {
		const load = vi.fn(async () => limits);
		const document = fakeDocument();
		let meta = $state<TranscriptMeta | null>(null);
		let usage!: PlanUsage;
		cleanup = $effect.root(() => {
			usage = new PlanUsage(load, () => meta, { document, window: undefined });
		});
		flushSync();
		return {
			load,
			document,
			usage,
			setMeta: (m: Partial<TranscriptMeta>) => {
				meta = { ...(meta as TranscriptMeta), ...m };
				flushSync();
			}
		};
	}

	it('loads on mount, then on each interval while visible', async () => {
		const { load, usage, document } = setup();
		await vi.waitFor(() => expect(usage.chips).toHaveLength(1));
		expect(usage.chips[0]).toMatchObject({ label: '5h', percent: 42 });

		await vi.advanceTimersByTimeAsync(PLAN_USAGE_REFRESH_MS);
		flushSync();
		expect(load).toHaveBeenCalledTimes(2);

		document.hidden = true;
		document.dispatchEvent(new Event('visibilitychange'));
		flushSync();
		await vi.advanceTimersByTimeAsync(PLAN_USAGE_REFRESH_MS);
		flushSync();
		expect(load).toHaveBeenCalledTimes(2);

		document.hidden = false;
		document.dispatchEvent(new Event('visibilitychange'));
		await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(3));
	});

	it('refreshes when a turn ends or the rate limit changes, not mid-turn', async () => {
		const { load, setMeta } = setup();
		await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(1));

		setMeta({ busy: true, rateLimit: null });
		expect(load).toHaveBeenCalledTimes(1);
		setMeta({ busy: false });
		await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(2));

		setMeta({
			rateLimit: { status: 'allowed_warning', resetsAt: null, kind: 'five_hour', utilization: 0.9 }
		});
		await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(3));
	});

	it('keeps the last figures when a check fails', async () => {
		const { load, usage } = setup();
		await vi.waitFor(() => expect(usage.chips).toHaveLength(1));
		load.mockRejectedValueOnce(new Error('404'));
		await usage.refresh();
		expect(usage.chips).toHaveLength(1);
	});
});
