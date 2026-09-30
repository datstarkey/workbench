import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { TranscriptMeta } from '$types/workbench';
import { TranscriptFeed } from './transcript-feed.svelte';

class FakeSocket {
	static last: FakeSocket | null = null;
	onopen: (() => void) | null = null;
	onmessage: ((e: { data: string }) => void) | null = null;
	onclose: (() => void) | null = null;
	closed = false;
	constructor(readonly url: string) {
		FakeSocket.last = this;
	}
	close() {
		this.closed = true;
	}
	emit(msg: unknown) {
		this.onmessage?.({ data: JSON.stringify(msg) });
	}
}

const meta: TranscriptMeta = {
	title: 'Fix keyboard',
	model: 'claude-opus-5-5',
	permissionMode: 'default',
	contextTokens: 1200,
	busy: true
};

const url = async (id: string) => `ws://test/claude/transcripts/${id}/ws`;

describe('TranscriptFeed', () => {
	beforeEach(() => {
		vi.useFakeTimers();
		vi.stubGlobal('WebSocket', FakeSocket);
		FakeSocket.last = null;
	});

	afterEach(() => {
		vi.useRealTimers();
		vi.unstubAllGlobals();
	});

	it('applies a snapshot then updates', async () => {
		const feed = new TranscriptFeed('sid', url);
		await vi.waitFor(() => expect(FakeSocket.last).not.toBeNull());
		const ws = FakeSocket.last!;
		expect(ws.url).toBe('ws://test/claude/transcripts/sid/ws');
		ws.onopen?.();
		expect(feed.status).toBe('live');

		ws.emit({
			t: 'snapshot',
			items: [{ kind: 'user', id: 'u', text: 'hi', timestamp: '' }],
			meta,
			truncated: true
		});
		ws.emit({ t: 'update', items: [{ kind: 'text', id: 'a', text: 'hello' }], meta });

		expect(feed.items.map((i) => i.id)).toEqual(['u', 'a']);
		expect(feed.meta?.title).toBe('Fix keyboard');
		expect(feed.truncated).toBe(true);
		feed.dispose();
	});

	it('reconnects after a drop but not after revoke or dispose', async () => {
		const feed = new TranscriptFeed('sid', url);
		await vi.waitFor(() => expect(FakeSocket.last).not.toBeNull());
		const first = FakeSocket.last!;
		first.onclose?.();
		expect(feed.status).toBe('connecting');
		await vi.advanceTimersByTimeAsync(2000);
		const second = FakeSocket.last!;
		expect(second).not.toBe(first);

		second.emit({ t: 'revoked' });
		expect(feed.status).toBe('closed');
		expect(second.closed).toBe(true);
		second.onclose?.();
		await vi.advanceTimersByTimeAsync(5000);
		expect(FakeSocket.last).toBe(second);
		feed.dispose();
	});
});
