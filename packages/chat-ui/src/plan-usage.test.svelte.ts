// @vitest-environment jsdom
import { flushSync } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RateLimitInfo, TranscriptMeta, UsageLimit } from '@workbench/types';
import { PLAN_USAGE_REFRESH_MS, PlanUsage, usePlanUsage } from './plan-usage.svelte';

const limits: UsageLimit[] = [{ label: 'session', percent: 42 }];

function fakeDocument() {
	const target = new EventTarget();
	return Object.assign(target, { hidden: false }) as unknown as Document & { hidden: boolean };
}

/** A mounted chat pane: its own meta, sharing the key's PlanUsage. */
function mountPane(
	key: string,
	load: (fresh: boolean) => Promise<UsageLimit[]>,
	document: Document
) {
	let meta = $state<TranscriptMeta | null>(null);
	let usage!: PlanUsage;
	const unmount = $effect.root(() => {
		usage = usePlanUsage(key, load, () => meta, { document, window: undefined });
	});
	flushSync();
	return {
		usage,
		unmount,
		setMeta(m: Partial<TranscriptMeta>) {
			meta = { ...(meta as TranscriptMeta), ...m };
			flushSync();
		}
	};
}

const inFuture = (): RateLimitInfo => ({
	status: 'allowed_warning',
	kind: 'five_hour',
	utilization: 0.9,
	resetsAt: Math.floor(Date.now() / 1000) + 3600
});

describe('PlanUsage', () => {
	const unmounts: (() => void)[] = [];
	let n = 0;
	let key: string;
	let document: Document & { hidden: boolean };
	beforeEach(() => {
		vi.useFakeTimers();
		key = `server|account-${n++}`;
		document = fakeDocument();
	});
	afterEach(() => {
		unmounts.splice(0).forEach((u) => u());
		vi.useRealTimers();
	});
	const mount = (load: (fresh: boolean) => Promise<UsageLimit[]>) => {
		const pane = mountPane(key, load, document);
		unmounts.push(pane.unmount);
		return pane;
	};

	it('loads on mount, then on each interval while visible', async () => {
		const load = vi.fn(async () => limits);
		const { usage } = mount(load);
		await vi.waitFor(() => expect(usage.chips).toHaveLength(1));
		expect(usage.chips[0]).toMatchObject({ label: '5h', percent: 42 });

		await vi.advanceTimersByTimeAsync(PLAN_USAGE_REFRESH_MS);
		expect(load).toHaveBeenCalledTimes(2);

		document.hidden = true;
		document.dispatchEvent(new Event('visibilitychange'));
		flushSync();
		await vi.advanceTimersByTimeAsync(PLAN_USAGE_REFRESH_MS);
		expect(load).toHaveBeenCalledTimes(2);

		document.hidden = false;
		document.dispatchEvent(new Event('visibilitychange'));
		await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(3));
		expect(load).toHaveBeenLastCalledWith(false);
	});

	it('shares one instance and timer per key, until the last pane unmounts', async () => {
		const load = vi.fn(async () => limits);
		const a = mount(load);
		const b = mount(load);
		expect(b.usage).toBe(a.usage);
		await vi.advanceTimersByTimeAsync(PLAN_USAGE_REFRESH_MS);
		expect(load).toHaveBeenCalledTimes(2);

		a.unmount();
		await vi.advanceTimersByTimeAsync(PLAN_USAGE_REFRESH_MS);
		expect(load).toHaveBeenCalledTimes(3);
		b.unmount();
		await vi.advanceTimersByTimeAsync(PLAN_USAGE_REFRESH_MS);
		expect(load).toHaveBeenCalledTimes(3);
		expect(mount(load).usage).not.toBe(a.usage);
	});

	it("asks for a fresh check when any pane's turn ends, not mid-turn", async () => {
		const load = vi.fn(async (_fresh: boolean) => limits);
		const a = mount(load);
		const b = mount(load);
		await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(1));

		b.setMeta({ busy: true, rateLimit: null });
		expect(load).toHaveBeenCalledTimes(1);
		b.setMeta({ busy: false });
		await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(2));
		expect(load).toHaveBeenLastCalledWith(true);

		a.setMeta({ busy: false, rateLimit: inFuture() });
		await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(3));
		expect(load).toHaveBeenLastCalledWith(true);
	});

	it('shows a rate_limit_event until a check starts after it', async () => {
		let resolve!: (l: UsageLimit[]) => void;
		const load = vi.fn(() => new Promise<UsageLimit[]>((r) => (resolve = r)));
		const pane = mount(load);
		await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(1));
		resolve(limits);
		await vi.waitFor(() => expect(pane.usage.chips[0]?.percent).toBe(42));

		pane.setMeta({ busy: true, rateLimit: inFuture() });
		await vi.waitFor(() => expect(pane.usage.chips[0].percent).toBe(90));
		pane.setMeta({ busy: false });
		await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(2));
		expect(pane.usage.chips[0].percent).toBe(90);
		resolve([{ label: 'session', percent: 91 }]);
		await vi.waitFor(() => expect(pane.usage.chips[0].percent).toBe(91));
	});

	it('queues one follow-up for requests made mid-check, fresh if any asked', async () => {
		const resolvers: ((l: UsageLimit[]) => void)[] = [];
		const load = vi.fn((_fresh: boolean) => new Promise<UsageLimit[]>((r) => resolvers.push(r)));
		const { usage } = mount(load);
		await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(1));
		void usage.refresh(true);
		void usage.refresh(false);
		expect(load).toHaveBeenCalledTimes(1);
		resolvers[0](limits);
		await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(2));
		expect(load).toHaveBeenLastCalledWith(true);
		resolvers[1](limits);
		await vi.advanceTimersByTimeAsync(0);
		expect(load).toHaveBeenCalledTimes(2);
	});

	it('keeps the last figures when a check fails', async () => {
		const load = vi.fn(async (_fresh: boolean) => limits);
		const { usage } = mount(load);
		await vi.waitFor(() => expect(usage.chips).toHaveLength(1));
		load.mockRejectedValueOnce(new Error('404'));
		await usage.refresh();
		expect(usage.chips).toHaveLength(1);
	});
});
