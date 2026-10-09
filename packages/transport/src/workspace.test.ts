import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { WorkspaceSnapshot } from '@workbench/types';
import { WorkspaceStream, sendWorkspaceCommand } from './workspace.ts';
import { createHttpTransport } from './http.ts';

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

const snap = (rev: number): WorkspaceSnapshot => ({ rev, workspaces: [] });
const settle = () => new Promise((r) => setTimeout(r, 0));

describe('WorkspaceStream', () => {
	let sources: FakeEventSource[];
	const open = (url: string) => {
		const s = new FakeEventSource(url);
		sources.push(s);
		return s as unknown as EventSource;
	};
	const latest = () => sources[sources.length - 1];

	beforeEach(() => {
		sources = [];
		vi.stubGlobal('EventSource', FakeEventSource);
	});
	afterEach(() => {
		vi.useRealTimers();
		vi.unstubAllGlobals();
	});

	it('follows /events/workspace with the token in the query', async () => {
		const stream = new WorkspaceStream(
			async () => ({ baseUrl: 'http://box:4317/', token: 't o' }),
			{ snapshot: vi.fn() },
			open
		);
		await settle();
		expect(latest().url).toBe('http://box:4317/events/workspace?token=t%20o');
		stream.stop();
		expect(latest().closed).toBe(true);
	});

	it('delivers newer revs only, the first of a connection marked fresh', async () => {
		const snapshot = vi.fn();
		const stream = new WorkspaceStream(async () => ({ baseUrl: 'http://h' }), { snapshot }, open);
		await settle();
		latest().emit('snapshot', snap(3));
		latest().emit('snapshot', snap(3));
		latest().emit('snapshot', snap(2));
		latest().emit('snapshot', snap(4));
		expect(snapshot.mock.calls).toEqual([
			[snap(3), true],
			[snap(4), false]
		]);
		stream.stop();
	});

	it('reconnects with backoff after an error, and a restarted server may restart rev', async () => {
		vi.useFakeTimers();
		const snapshot = vi.fn();
		const status = vi.fn();
		const stream = new WorkspaceStream(
			async () => ({ baseUrl: 'http://h' }),
			{ snapshot, status },
			open
		);
		await vi.advanceTimersByTimeAsync(0);
		latest().emit('snapshot', snap(9));
		expect(status).toHaveBeenLastCalledWith(true);
		latest().emit('error');
		expect(status).toHaveBeenLastCalledWith(false);
		expect(sources).toHaveLength(1);
		await vi.advanceTimersByTimeAsync(1000);
		expect(sources).toHaveLength(2);
		latest().emit('error');
		await vi.advanceTimersByTimeAsync(1000);
		expect(sources).toHaveLength(2);
		await vi.advanceTimersByTimeAsync(1000);
		expect(sources).toHaveLength(3);
		latest().emit('snapshot', snap(1));
		expect(snapshot).toHaveBeenLastCalledWith(snap(1), true);
		stream.stop();
	});

	it('treats a silent stream as dead', async () => {
		vi.useFakeTimers();
		const stream = new WorkspaceStream(
			async () => ({ baseUrl: 'http://h' }),
			{ snapshot: vi.fn() },
			open
		);
		await vi.advanceTimersByTimeAsync(0);
		await vi.advanceTimersByTimeAsync(45_000);
		expect(sources[0].closed).toBe(true);
		await vi.advanceTimersByTimeAsync(1000);
		expect(sources).toHaveLength(2);
		stream.stop();
	});
});

describe('workspace commands', () => {
	afterEach(() => vi.unstubAllGlobals());

	it('POSTs the command with the bearer token', async () => {
		const fetchSpy = vi.fn(async () => new Response(JSON.stringify({ rev: 5, paneId: 'p1' })));
		vi.stubGlobal('fetch', fetchSpy);
		const transport = createHttpTransport({ baseUrl: 'http://box:4317/', token: 'tok' });
		const result = await transport.workspaceCommand({ type: 'closePane', paneId: 'p1' });
		expect(result).toEqual({ rev: 5, paneId: 'p1' });
		const [url, init] = fetchSpy.mock.calls[0] as unknown as [string, RequestInit];
		expect(url).toBe('http://box:4317/workspace/commands');
		expect(init.method).toBe('POST');
		expect((init.headers as Record<string, string>).authorization).toBe('Bearer tok');
		expect(JSON.parse(String(init.body))).toEqual({ type: 'closePane', paneId: 'p1' });
	});

	it('throws the server reason for a refused command', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn(
				async () =>
					new Response(JSON.stringify({ rev: 2, error: "Native terminals can't be split" }), {
						status: 400
					})
			)
		);
		await expect(
			sendWorkspaceCommand(
				{ baseUrl: 'http://h' },
				{ type: 'split', tabId: 't', direction: 'vertical' }
			)
		).rejects.toThrow("Native terminals can't be split");
	});
});
