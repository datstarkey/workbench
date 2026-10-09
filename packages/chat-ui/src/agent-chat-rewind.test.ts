import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentKind, TranscriptItem, TranscriptMeta } from '@workbench/types';
import type { AgentApi } from './agent-api';
import { AgentChat } from './agent-chat.svelte';

class FakeSocket {
	static OPEN = 1;
	static all: FakeSocket[] = [];
	readyState = FakeSocket.OPEN;
	sent: Record<string, unknown>[] = [];
	onmessage: ((e: { data: string }) => void) | null = null;
	onclose: ((e?: { code?: number }) => void) | null = null;
	constructor(readonly url: string) {
		FakeSocket.all.push(this);
	}
	send(data: string) {
		this.sent.push(JSON.parse(data));
	}
	close() {
		this.readyState = 3;
	}
	emit(msg: unknown) {
		this.onmessage?.({ data: JSON.stringify(msg) });
	}
}

const meta = (busy = false): TranscriptMeta => ({
	title: null,
	model: null,
	permissionMode: 'default',
	contextTokens: null,
	busy,
	tasks: [],
	retry: null,
	rateLimit: null,
	models: [],
	modelChoice: null,
	effort: null
});

const items: TranscriptItem[] = [
	{ kind: 'user', id: 'u1', text: 'first', timestamp: '' },
	{ kind: 'text', id: 'a1', text: 'one' },
	{ kind: 'user', id: 'u2', text: 'second', timestamp: '' },
	{ kind: 'text', id: 'a2', text: 'two' }
];

const api: AgentApi = {
	socketUrl: async (id) => `ws://test/agent/claude/${id}/ws`
};

async function connected(agent: AgentKind = 'claude') {
	const chat = new AgentChat({ projectPath: '/repo', sessionId: 'sid', agent }, api);
	await vi.waitFor(() => expect(FakeSocket.all).toHaveLength(1));
	const ws = FakeSocket.all[0];
	ws.emit({
		t: 'snapshot',
		sessionId: 'sid',
		start: 0,
		items,
		meta: meta(),
		commands: [],
		exited: false
	});
	return { chat, ws };
}

const filesPreview = {
	canRewind: true,
	filesChanged: ['/repo/a.txt'],
	insertions: 1,
	deletions: 2
};

describe('AgentChat rewind', () => {
	beforeEach(() => {
		vi.stubGlobal('WebSocket', FakeSocket);
		FakeSocket.all = [];
	});
	afterEach(() => vi.unstubAllGlobals());

	it('previews the file restore, then rewinds code and conversation', async () => {
		const { chat, ws } = await connected();
		expect(chat.canRewind).toBe(true);

		const begun = chat.beginRewind('u2', 'second');
		expect(chat.rewind?.phase).toBe('checking');
		expect(ws.sent.at(-1)).toEqual({
			t: 'rewind',
			messageId: 'u2',
			code: true,
			conversation: false,
			dryRun: true
		});
		ws.emit({ t: 'rewind', messageId: 'u2', dryRun: true, files: filesPreview, error: null });
		await begun;
		expect(chat.rewind).toMatchObject({ phase: 'ready', files: filesPreview, error: null });

		const applied = chat.confirmRewind(true, true);
		expect(chat.rewind?.phase).toBe('working');
		expect(ws.sent.at(-1)).toMatchObject({ code: true, conversation: true, dryRun: false });
		ws.emit({
			t: 'rewind',
			messageId: 'u2',
			dryRun: false,
			files: { canRewind: true },
			error: null
		});
		expect(await applied).toBe('second');
		expect(chat.rewind).toBeNull();
		expect(chat.items.map((i) => i.id)).toEqual(['u1', 'a1']);

		// The server restarted the process: attach again at once.
		ws.emit({ t: 'replaced' });
		await vi.waitFor(() => expect(FakeSocket.all).toHaveLength(2));
	});

	it('keeps the panel open with the error when the rewind fails', async () => {
		const { chat, ws } = await connected();
		const begun = chat.beginRewind('u2', 'second');
		ws.emit({ t: 'rewind', messageId: 'u2', dryRun: true, files: filesPreview, error: null });
		await begun;
		const applied = chat.confirmRewind(false, true);
		ws.emit({
			t: 'rewind',
			messageId: 'u2',
			dryRun: false,
			files: null,
			error: 'Wait for the turn'
		});
		expect(await applied).toBeNull();
		expect(chat.rewind).toMatchObject({ phase: 'ready', error: 'Wait for the turn' });
		expect(chat.items).toHaveLength(4);
	});

	it('a closed socket settles a pending preview', async () => {
		const { chat, ws } = await connected();
		const begun = chat.beginRewind('u2', 'second');
		ws.onclose?.();
		await begun;
		expect(chat.rewind).toMatchObject({ phase: 'ready', error: 'Not connected.' });
	});

	it('is not offered while a turn runs, nor for Codex', async () => {
		const { chat, ws } = await connected();
		ws.emit({ t: 'update', changes: [], meta: meta(true) });
		expect(chat.canRewind).toBe(false);
		await chat.beginRewind('u2', 'second');
		expect(chat.rewind).toBeNull();

		FakeSocket.all = [];
		const codex = await connected('codex');
		expect(codex.chat.canRewind).toBe(false);
	});
});
