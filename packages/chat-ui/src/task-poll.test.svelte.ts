import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TaskPoll } from './task-poll.svelte';

describe('TaskPoll', () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => vi.useRealTimers());

	it('polls while live and once more after the task ends', async () => {
		let n = 0;
		let live = true;
		const fetch = vi.fn(async () => ++n);
		const poll = new TaskPoll(fetch, () => live, 1000);
		const stop = poll.start();
		await vi.advanceTimersByTimeAsync(0);
		expect(poll.loaded).toBe(true);
		expect(poll.value).toBe(1);

		await vi.advanceTimersByTimeAsync(1000);
		expect(poll.value).toBe(2);
		live = false;
		await vi.advanceTimersByTimeAsync(1000);
		expect(poll.value).toBe(3);
		await vi.advanceTimersByTimeAsync(5000);
		expect(fetch).toHaveBeenCalledTimes(3);
		stop();
	});

	it('fetches a finished task once', async () => {
		const fetch = vi.fn(async () => 'done');
		const stop = new TaskPoll(fetch, () => false, 1000).start();
		await vi.advanceTimersByTimeAsync(5000);
		expect(fetch).toHaveBeenCalledTimes(1);
		stop();
	});

	it('runs one more fetch when the task ends while a slow one is out', async () => {
		let live = true;
		let resolve: (v: string) => void = () => {};
		const fetch = vi.fn(() => new Promise<string | null>((r) => (resolve = r)));
		const poll = new TaskPoll(fetch, () => live, 1000);
		const stop = poll.start();
		live = false;
		await vi.advanceTimersByTimeAsync(3000);
		expect(fetch).toHaveBeenCalledTimes(1);
		resolve('stale');
		await vi.advanceTimersByTimeAsync(0);
		expect(fetch).toHaveBeenCalledTimes(2);
		resolve('final');
		await vi.advanceTimersByTimeAsync(5000);
		expect(poll.value).toBe('final');
		expect(fetch).toHaveBeenCalledTimes(2);
		stop();
	});

	it('drops a reply that lands after stop', async () => {
		let resolve: (v: string) => void = () => {};
		const fetch = vi.fn(() => new Promise<string | null>((r) => (resolve = r)));
		const poll = new TaskPoll(fetch, () => true, 1000);
		const stop = poll.start();
		stop();
		resolve('late');
		await vi.advanceTimersByTimeAsync(0);
		expect(poll.loaded).toBe(false);
		expect(poll.value).toBeNull();
	});
});
