import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { StartAgentBody, TranscriptMeta } from '$types/workbench';
import type { AgentApi } from './agent-api';
import { AgentChat } from './agent-chat.svelte';

class FakeSocket {
	static OPEN = 1;
	static last: FakeSocket | null = null;
	readyState = FakeSocket.OPEN;
	sent: unknown[] = [];
	onmessage: ((e: { data: string }) => void) | null = null;
	onclose: (() => void) | null = null;
	constructor(readonly url: string) {
		FakeSocket.last = this;
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
	title: 'Fix keyboard',
	model: 'claude-opus-5-5[1m]',
	permissionMode: 'default',
	contextTokens: 1200,
	busy,
	tasks: [],
	retry: null,
	rateLimit: null
});

const body: StartAgentBody = { projectPath: '/repo', sessionId: 'sid', paneId: 'p1' };

function fakeApi(start = vi.fn<AgentApi['start']>().mockResolvedValue()): AgentApi {
	return { start, socketUrl: async (id) => `ws://test/agent/claude/${id}/ws` };
}

async function connected(api = fakeApi()) {
	const chat = new AgentChat(body, api);
	await vi.waitFor(() => expect(FakeSocket.last).not.toBeNull());
	const ws = FakeSocket.last!;
	ws.emit({ t: 'snapshot', sessionId: 'sid', start: 0, items: [], meta: meta(), exited: false });
	return { chat, ws };
}

describe('AgentChat', () => {
	beforeEach(() => {
		vi.useFakeTimers();
		vi.stubGlobal('WebSocket', FakeSocket);
		FakeSocket.last = null;
	});
	afterEach(() => {
		vi.useRealTimers();
		vi.unstubAllGlobals();
	});

	it('starts the session, then streams updates into the chat', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue();
		const { chat, ws } = await connected(fakeApi(start));
		expect(start).toHaveBeenCalledWith(body);
		expect(chat.status).toBe('live');

		ws.emit({
			t: 'update',
			changes: [[0, { kind: 'text', id: 'm1:0', text: 'Hel' }]],
			meta: meta(true)
		});
		ws.emit({
			t: 'update',
			changes: [[0, { kind: 'text', id: 'm1:0', text: 'Hello' }]],
			meta: meta(true)
		});
		expect(chat.items).toEqual([{ kind: 'text', id: 'm1:0', text: 'Hello' }]);
		expect(chat.busySince).not.toBeNull();

		ws.emit({ t: 'update', changes: [], meta: meta(false) });
		expect(chat.busySince).toBeNull();
		chat.dispose();
	});

	it('shows a prompt at once and drops it when Claude echoes it', async () => {
		const { chat, ws } = await connected();
		expect(chat.prompt('  fix the build  ')).toBe(true);
		expect(ws.sent).toEqual([{ t: 'prompt', text: 'fix the build' }]);
		expect(chat.pending.map((p) => p.text)).toEqual(['fix the build']);

		ws.emit({
			t: 'update',
			changes: [[0, { kind: 'user', id: 'u1', text: 'fix the build', timestamp: '' }]],
			meta: meta(true)
		});
		expect(chat.pending).toEqual([]);
		chat.dispose();
	});

	it('sends approvals, interrupts and mode changes', async () => {
		const { chat, ws } = await connected();
		chat.approve('req-1', 'alwaysAllow');
		chat.approve('q-1', 'allow', { 'Which library?': 'dayjs' });
		chat.interrupt();
		chat.setMode('plan');
		expect(ws.sent).toEqual([
			{ t: 'approve', requestId: 'req-1', decision: 'alwaysAllow' },
			{ t: 'approve', requestId: 'q-1', decision: 'allow', answers: { 'Which library?': 'dayjs' } },
			{ t: 'interrupt' },
			{ t: 'mode', mode: 'plan' }
		]);
		chat.dispose();
	});

	it('reports a failed start and can try again', async () => {
		const start = vi
			.fn<AgentApi['start']>()
			.mockRejectedValueOnce(new Error('Chat mode does not run inside the sandbox runtime yet.'))
			.mockResolvedValue();
		const chat = new AgentChat(body, fakeApi(start));
		await vi.waitFor(() => expect(chat.status).toBe('failed'));
		expect(chat.error).toContain('sandbox');

		await chat.open();
		expect(start).toHaveBeenCalledTimes(2);
		expect(FakeSocket.last).not.toBeNull();
		chat.dispose();
	});

	it('marks the session ended on exit and refuses to send', async () => {
		const { chat, ws } = await connected();
		ws.emit({ t: 'exit', code: 1, message: 'Not logged in' });
		expect(chat.status).toBe('exited');
		expect(chat.error).toBe('Not logged in');
		ws.readyState = 3;
		expect(chat.prompt('hello?')).toBe(false);
		expect(chat.notice).toBeTruthy();
		chat.dispose();
	});

	it('reconnects after a dropped socket, starting the session again', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue();
		const { chat, ws } = await connected(fakeApi(start));
		ws.onclose?.();
		expect(chat.status).toBe('reconnecting');
		await vi.advanceTimersByTimeAsync(1500);
		expect(start).toHaveBeenCalledTimes(2);
		expect(FakeSocket.last).not.toBe(ws);
		chat.dispose();
	});

	it('tells the pane when Claude starts and stops waiting on you', async () => {
		const { chat, ws } = await connected();
		const calls: boolean[] = [];
		chat.onNeedsYou = (waiting) => calls.push(waiting);
		const approval = {
			kind: 'approval',
			id: 'r1',
			tool: 'Bash',
			input: { command: 'ls' },
			canAlwaysAllow: false,
			expired: false
		} as const;
		ws.emit({ t: 'update', changes: [[0, approval]], meta: meta(true) });
		ws.emit({ t: 'update', changes: [[0, approval]], meta: meta(true) });
		ws.emit({ t: 'update', changes: [[0, { ...approval, decision: 'allow' }]], meta: meta(true) });
		expect(calls).toEqual([true, false]);
		chat.dispose();
	});

	it("fetches a tool's whole output on request", async () => {
		const { chat, ws } = await connected();
		const full = chat.fullOutput('toolu_1');
		expect(ws.sent).toEqual([{ t: 'output', toolId: 'toolu_1' }]);
		ws.emit({ t: 'output', toolId: 'toolu_1', text: 'all of it' });
		await expect(full).resolves.toBe('all of it');
		chat.dispose();
	});

	it('follows /clear to the new session id', async () => {
		const { chat, ws } = await connected();
		ws.emit({
			t: 'snapshot',
			sessionId: 'new-id',
			start: 0,
			items: [],
			meta: meta(),
			exited: false
		});
		expect(chat.sessionId).toBe('new-id');
		chat.dispose();
	});
});
