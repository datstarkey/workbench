import { tick } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MobileClient } from './client.svelte.ts';
import {
	CONNECT_ROUTES,
	jsonResponse,
	routeFetch,
	stubLocalStorage,
	TOKEN
} from './test-helpers.ts';

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));

class FakeEventSource extends EventTarget {
	closed = false;
	constructor(readonly url: string) {
		super();
	}
	close() {
		this.closed = true;
	}
	emit(type: string, data?: unknown) {
		this.dispatchEvent(
			data === undefined ? new Event(type) : new MessageEvent(type, { data: JSON.stringify(data) })
		);
	}
}

const chat = { sessionId: 's1', agent: 'claude', projectPath: '/p', previousIds: [] };
const terminal = { id: 't1', cwd: '/p', createdAt: 0, alive: true };

describe('home event stream', () => {
	let sources: FakeEventSource[];
	let fetchSpy: ReturnType<typeof routeFetch>;
	let stop: () => void;

	beforeEach(() => {
		stubLocalStorage();
		sources = [];
		vi.stubGlobal('EventSource', FakeEventSource);
	});
	afterEach(() => {
		stop?.();
		vi.useRealTimers();
		vi.restoreAllMocks();
		vi.unstubAllGlobals();
	});

	const latest = () => sources[sources.length - 1];
	const listCalls = () =>
		fetchSpy.mock.calls.filter(([url]) => new URL(url).pathname === '/remote/terminals').length;

	async function watching(routes: Parameters<typeof routeFetch>[0] = {}) {
		fetchSpy = routeFetch({
			...CONNECT_ROUTES,
			'/remote/terminals': () => jsonResponse([]),
			'/agent': () => jsonResponse([]),
			...routes
		});
		const c = new MobileClient(undefined, (url) => {
			const source = new FakeEventSource(url);
			sources.push(source);
			return source as unknown as EventSource;
		});
		c.url = 'box:4317';
		c.token = TOKEN;
		await c.connect();
		vi.useFakeTimers();
		stop = c.watch();
		await tick();
		return c;
	}

	it('streams the home lists instead of polling them', async () => {
		const c = await watching();
		expect(latest().url).toBe(`http://box:4317/events/home?token=${TOKEN}`);
		latest().emit('agents', [chat]);
		latest().emit('terminals', [terminal]);
		expect(c.chats.map((x) => x.sessionId)).toEqual(['s1']);
		expect(c.terminals.map((t) => t.id)).toEqual(['t1']);

		const before = listCalls();
		await vi.advanceTimersByTimeAsync(12_000);
		expect(listCalls()).toBe(before);
	});

	it('polls while the stream is down, and retries it with backoff', async () => {
		const c = await watching({ '/remote/terminals': () => jsonResponse([terminal]) });
		latest().emit('error');
		expect(latest().closed).toBe(true);

		await vi.advanceTimersByTimeAsync(4000);
		expect(c.terminals.map((t) => t.id)).toEqual(['t1']);
		expect(sources).toHaveLength(2);

		// The retry fails too (an older host): the next one waits twice as long.
		latest().emit('error');
		await vi.advanceTimersByTimeAsync(3999);
		expect(sources).toHaveLength(2);
		await vi.advanceTimersByTimeAsync(1);
		expect(sources).toHaveLength(3);

		// Once it delivers, polling stops.
		latest().emit('agents', []);
		const before = listCalls();
		await vi.advanceTimersByTimeAsync(8000);
		expect(listCalls()).toBe(before);
	});

	it('treats a stream silent past its heartbeat as dead', async () => {
		await watching();
		latest().emit('ping', {});
		await vi.advanceTimersByTimeAsync(44_000);
		expect(latest().closed).toBe(false);
		latest().emit('ping', {});
		await vi.advanceTimersByTimeAsync(45_000);
		expect(sources[0].closed).toBe(true);
	});

	it('closes the stream off Home and in the background, and reopens it on return', async () => {
		const c = await watching();
		const first = latest();
		c.openChat({ sessionId: 's1', projectPath: '/p', name: 'p' });
		await tick();
		expect(first.closed).toBe(true);

		c.closeChat();
		await tick();
		expect(sources).toHaveLength(2);
		expect(latest().closed).toBe(false);

		vi.spyOn(document, 'hidden', 'get').mockReturnValue(true);
		document.dispatchEvent(new Event('visibilitychange'));
		await tick();
		expect(latest().closed).toBe(true);

		vi.spyOn(document, 'hidden', 'get').mockReturnValue(false);
		document.dispatchEvent(new Event('visibilitychange'));
		await tick();
		expect(sources).toHaveLength(3);
	});

	it('stops streaming when the watch stops or the machine disconnects', async () => {
		const c = await watching();
		c.disconnect();
		await tick();
		expect(latest().closed).toBe(true);
		c.url = 'box:4317';
		await c.connect();
		await tick();
		stop();
		expect(latest().closed).toBe(true);
	});

	it('runs one poll at a time, and shows the machine offline when a list request times out', async () => {
		const aborts: AbortController[] = [];
		vi.spyOn(AbortSignal, 'timeout').mockImplementation(() => {
			const controller = new AbortController();
			aborts.push(controller);
			return controller.signal;
		});
		const c = await watching();
		latest().emit('error');
		fetchSpy.mockImplementation(
			(_url: string, init?: RequestInit) =>
				new Promise((_, reject) =>
					init?.signal?.addEventListener('abort', () => reject(new Error('timeout')))
				)
		);
		const before = fetchSpy.mock.calls.length;
		await vi.advanceTimersByTimeAsync(12_000);
		const hung = fetchSpy.mock.calls.slice(before).map(([url]) => new URL(url).pathname);
		expect(hung.filter((p) => p === '/remote/terminals')).toHaveLength(1);

		aborts[aborts.length - 1].abort();
		await vi.advanceTimersByTimeAsync(0);
		expect(c.online).toBe(false);
	});

	it('drops a poll response that a streamed list overtook', async () => {
		let answer: (r: Response) => void = () => {};
		const c = await watching();
		latest().emit('error');
		fetchSpy.mockImplementation((url: string) =>
			new URL(url).pathname === '/remote/terminals'
				? new Promise<Response>((done) => (answer = done))
				: Promise.resolve(jsonResponse([]))
		);
		await vi.advanceTimersByTimeAsync(4000);
		latest().emit('terminals', [terminal]);
		answer(jsonResponse([]));
		await vi.advanceTimersByTimeAsync(0);
		expect(c.terminals.map((t) => t.id)).toEqual(['t1']);
	});

	it('polls again after a round that never settles', async () => {
		const c = await watching();
		latest().emit('error');
		fetchSpy.mockImplementation(() => new Promise<Response>(() => {}));
		vi.spyOn(c.agents, 'list').mockReturnValue(new Promise(() => {}));
		const before = fetchSpy.mock.calls.length;
		await vi.advanceTimersByTimeAsync(16_000);
		const polls = fetchSpy.mock.calls
			.slice(before)
			.filter(([url]) => new URL(url).pathname === '/remote/terminals');
		expect(polls.length).toBe(2);
	});
});
