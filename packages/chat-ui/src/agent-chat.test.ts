import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { StartAgentBody, TranscriptMeta } from '@workbench/types';
import { NeedsTrustError, type AgentApi } from './agent-api';
import { AgentChat } from './agent-chat.svelte';
import { ChatDraft } from './chat-draft.svelte';

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
	rateLimit: null,
	models: [],
	modelChoice: null,
	effort: null
});

const body: StartAgentBody = { projectPath: '/repo', sessionId: 'sid', paneId: 'p1' };

/** Like the server: resumes the id it's given; a Codex start without one gets a new thread. */
function fakeApi(
	start = vi.fn<AgentApi['start']>(async (b) => b.sessionId ?? 'new-thread')
): AgentApi {
	return { start, socketUrl: async (id) => `ws://test/agent/claude/${id}/ws` };
}

async function connected(api = fakeApi(), startBody = body) {
	const chat = new AgentChat(startBody, api);
	await vi.waitFor(() => expect(FakeSocket.last).not.toBeNull());
	const ws = FakeSocket.last!;
	ws.emit({
		t: 'snapshot',
		sessionId: 'sid',
		start: 0,
		items: [],
		meta: meta(),
		commands: [],
		exited: false
	});
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
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('sid');
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

	it('matches a slash command echoed with its spaces collapsed', async () => {
		const { chat, ws } = await connected();
		chat.prompt('/code-review  check it');
		ws.emit({
			t: 'update',
			changes: [[0, { kind: 'user', id: 'u1', text: '/code-review check it', timestamp: '' }]],
			meta: meta(true)
		});
		expect(chat.pending).toEqual([]);
		chat.dispose();
	});

	it('matches a slash command alias echoed under its hyphenated name', async () => {
		const { chat, ws } = await connected();
		chat.prompt('/design consent');
		ws.emit({
			t: 'update',
			changes: [[0, { kind: 'user', id: 'u1', text: '/design-consent', timestamp: '' }]],
			meta: meta()
		});
		expect(chat.pending).toEqual([]);
		chat.dispose();
	});

	it('drops a slash command the CLI only answers with a notice', async () => {
		const { chat, ws } = await connected();
		ws.emit({
			t: 'update',
			changes: [[0, { kind: 'notice', id: 'n0', text: 'Conversation compacted' }]],
			meta: meta()
		});
		chat.prompt('/design-login');
		chat.prompt('fix the build');
		ws.emit({ t: 'update', changes: [], meta: meta() });
		expect(chat.pending.map((p) => p.text)).toEqual(['/design-login', 'fix the build']);

		ws.emit({
			t: 'update',
			changes: [
				[
					1,
					{ kind: 'notice', id: 'n1', text: "/design-login isn't available in this environment." }
				]
			],
			meta: meta()
		});
		expect(chat.pending.map((p) => p.text)).toEqual(['fix the build']);
		chat.dispose();
	});

	it('offers Stop while a background agent runs, not for a background shell alone', async () => {
		const { chat, ws } = await connected();
		const task = (kind: string) => ({
			id: kind,
			kind,
			description: kind,
			status: 'running' as const,
			background: true,
			toolUses: 0,
			tokens: 0,
			durationMs: 0
		});
		expect(chat.stoppable).toBe(false);
		ws.emit({ t: 'update', changes: [], meta: { ...meta(), tasks: [task('local_bash')] } });
		expect(chat.stoppable).toBe(false);
		ws.emit({ t: 'update', changes: [], meta: { ...meta(), tasks: [task('agent')] } });
		expect(chat.stoppable).toBe(true);
		chat.dispose();
	});

	it('keeps a slash command queued behind a busy turn until the turn ends', async () => {
		const { chat, ws } = await connected();
		chat.prompt('/design-login');
		ws.emit({
			t: 'update',
			changes: [[0, { kind: 'notice', id: 'n1', text: 'Interrupted' }]],
			meta: meta(true)
		});
		expect(chat.pending.map((p) => p.text)).toEqual(['/design-login']);
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

	it('drops a /clear and re-bases prompts queued after it when the session re-keys', async () => {
		const { chat, ws } = await connected();
		ws.emit({
			t: 'update',
			changes: [[0, { kind: 'user', id: 'u1', text: 'hello', timestamp: '' }]],
			meta: meta(true)
		});
		chat.prompt('/clear');
		chat.prompt('next thing');
		expect(chat.pending.map((p) => p.text)).toEqual(['/clear', 'next thing']);

		ws.emit({
			t: 'snapshot',
			sessionId: 'new-session',
			start: 0,
			items: [],
			meta: meta(),
			commands: [],
			exited: false
		});
		expect(chat.pending.map((p) => p.text)).toEqual(['next thing']);
		ws.emit({
			t: 'update',
			changes: [[0, { kind: 'user', id: 'u2', text: 'next thing', timestamp: '' }]],
			meta: meta(true)
		});
		expect(chat.pending).toEqual([]);
		chat.dispose();
	});

	it('sends pasted images with the prompt and keeps their previews', async () => {
		const { chat, ws } = await connected();
		const png = { mediaType: 'image/png', data: 'iVBORw==', name: 'shot.png' };
		expect(chat.prompt('', [png])).toBe(true);
		expect(ws.sent).toEqual([
			{ t: 'prompt', text: '', images: [{ mediaType: 'image/png', data: 'iVBORw==' }] }
		]);
		ws.emit({
			t: 'update',
			changes: [[0, { kind: 'user', id: 'u1', text: '', timestamp: '', images: 1 }]],
			meta: meta(true)
		});
		expect(chat.pending).toEqual([]);
		expect(chat.imagePreviews.u1).toEqual(['data:image/png;base64,iVBORw==']);
		chat.dispose();
	});

	it('sends attached files whole and shows their names while pending', async () => {
		const { chat, ws } = await connected();
		const pdf = { mediaType: 'application/pdf' as const, data: 'JVBERg==', name: 'report.pdf' };
		expect(chat.prompt('summarise', [], [pdf])).toBe(true);
		expect(ws.sent).toEqual([{ t: 'prompt', text: 'summarise', files: [pdf] }]);
		expect(chat.pending[0].files).toEqual(['report.pdf']);
		chat.dispose();
	});

	it.each(['claude', 'codex'] as const)(
		'%s file-only prompts settle when the server echoes uploaded file references',
		async (agent) => {
			const chat = new AgentChat({ ...body, agent }, fakeApi());
			await vi.waitFor(() => expect(FakeSocket.last).not.toBeNull());
			const ws = FakeSocket.last!;
			ws.emit({
				t: 'snapshot',
				sessionId: 'sid',
				start: 0,
				items: [],
				meta: meta(),
				commands: [],
				exited: false
			});
			const file = { mediaType: 'text/plain' as const, data: 'hello', name: 'notes.txt' };
			expect(chat.prompt('', [], [file])).toBe(true);
			expect(ws.sent).toEqual([{ t: 'prompt', text: '', files: [file] }]);
			expect(chat.pending[0].files).toEqual(['notes.txt']);
			ws.emit({
				t: 'update',
				changes: [
					[
						0,
						{
							kind: 'user',
							id: 'u1',
							text: '@"/tmp/upload/notes.txt"\n\nRead the attached file.',
							timestamp: ''
						}
					]
				],
				meta: meta(true)
			});
			expect(chat.pending).toEqual([]);
			chat.dispose();
		}
	);

	it('lists the cwd for @ mentions, reusing the list for a while', async () => {
		const files = vi.fn<NonNullable<AgentApi['files']>>(async () => ['src/main.rs']);
		const chat = new AgentChat({ ...body, worktreePath: '/repo-wt' }, { ...fakeApi(), files });
		expect(await chat.listFiles()).toEqual(['src/main.rs']);
		await chat.listFiles();
		expect(files).toHaveBeenCalledTimes(1);
		expect(files).toHaveBeenCalledWith({ projectPath: '/repo', worktreePath: '/repo-wt' });
		vi.advanceTimersByTime(31_000);
		files.mockRejectedValueOnce(new Error('not a git repository'));
		expect(await chat.listFiles()).toEqual([]);
		chat.dispose();
		const old = new AgentChat(body, fakeApi());
		expect(await old.listFiles()).toEqual([]);
		old.dispose();
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
			.mockResolvedValue('sid');
		const chat = new AgentChat(body, fakeApi(start));
		await vi.waitFor(() => expect(chat.status).toBe('failed'));
		expect(chat.error).toContain('sandbox');

		await chat.open();
		expect(start).toHaveBeenCalledTimes(2);
		expect(FakeSocket.last).not.toBeNull();
		chat.dispose();
	});

	it('asks to trust the folder, then starts again answering Claude Code', async () => {
		const start = vi
			.fn<AgentApi['start']>()
			.mockRejectedValueOnce(new NeedsTrustError('/repo'))
			.mockResolvedValue('sid');
		const chat = new AgentChat(body, fakeApi(start));
		await vi.waitFor(() => expect(chat.status).toBe('trust'));
		expect(chat.trustPath).toBe('/repo');
		chat.reconnect();
		expect(start).toHaveBeenCalledTimes(1);

		await chat.trustFolder();
		expect(start).toHaveBeenLastCalledWith({ ...body, trustFolder: true });
		expect(chat.trustPath).toBeNull();
		expect(FakeSocket.last).not.toBeNull();
		chat.dispose();
	});

	it('re-attaches an attach-only chat, and only a restart after it ended starts it here', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('sid');
		const attach = { ...body, attachOnly: true };
		const chat = new AgentChat(attach, fakeApi(start));
		const onTakeOver = vi.fn();
		chat.onTakeOver = onTakeOver;
		await vi.waitFor(() => expect(FakeSocket.last).not.toBeNull());
		FakeSocket.last!.emit({
			t: 'snapshot',
			sessionId: 'sid',
			start: 0,
			items: [],
			meta: meta(),
			commands: [],
			exited: false
		});

		await chat.open();
		expect(start).toHaveBeenLastCalledWith(attach);
		expect(onTakeOver).not.toHaveBeenCalled();

		FakeSocket.last!.emit({ t: 'exit', code: 0, message: null });
		await chat.open();
		expect(onTakeOver).toHaveBeenCalledOnce();
		expect(start).toHaveBeenLastCalledWith({ ...body, attachOnly: false });
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

	it('reports an End, but not a crash, as ended', async () => {
		const { chat, ws } = await connected();
		const onEnded = vi.fn();
		chat.onEnded = onEnded;
		ws.emit({ t: 'exit', code: 1, message: 'boom' });
		expect(onEnded).not.toHaveBeenCalled();
		ws.emit({ t: 'exit', code: null, message: null, ended: true });
		expect(onEnded).toHaveBeenCalledOnce();
		chat.dispose();
	});

	it('reports a joined chat someone ended as ended, but not one that exited', async () => {
		for (const ended of [false, true]) {
			const start = vi
				.fn<AgentApi['start']>()
				.mockRejectedValue(Object.assign(new Error('gone'), { status: 404, ended }));
			const chat = new AgentChat({ ...body, attachOnly: true }, fakeApi(start));
			const onEnded = vi.fn();
			chat.onEnded = onEnded;
			await vi.waitFor(() => expect(chat.status).toBe('failed'));
			expect(onEnded).toHaveBeenCalledTimes(ended ? 1 : 0);
			chat.dispose();
		}
	});

	it('re-attaches after a dropped socket without starting a new process', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('sid');
		const { chat, ws } = await connected(fakeApi(start));
		ws.onclose?.();
		expect(chat.status).toBe('reconnecting');
		await vi.advanceTimersByTimeAsync(1500);
		expect(start).toHaveBeenCalledTimes(2);
		expect(start).toHaveBeenLastCalledWith({ ...body, attachOnly: true });
		expect(FakeSocket.last).not.toBe(ws);
		chat.dispose();
	});

	it('backs off between failed reconnects, then gives up until Restart starts it', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('sid');
		const { chat, ws } = await connected(fakeApi(start));
		start.mockRejectedValue(new Error('Failed to fetch'));
		ws.onclose?.();
		const delays = [1500, 3000, 6000, 12000, 24000, 30000, 30000, 30000];
		for (const [i, delay] of delays.entries()) {
			expect(chat.status).toBe('reconnecting');
			await vi.advanceTimersByTimeAsync(delay - 1);
			expect(start).toHaveBeenCalledTimes(i + 1);
			await vi.advanceTimersByTimeAsync(1);
			expect(start).toHaveBeenCalledTimes(i + 2);
			expect(start).toHaveBeenLastCalledWith({ ...body, attachOnly: true });
		}
		expect(chat.status).toBe('exited');
		expect(chat.error).toContain('Lost the connection');
		await vi.advanceTimersByTimeAsync(120_000);
		expect(start).toHaveBeenCalledTimes(delays.length + 1);

		start.mockResolvedValue('sid');
		await chat.open();
		expect(start).toHaveBeenLastCalledWith(body);
		expect(FakeSocket.last).not.toBe(ws);
		chat.dispose();
	});

	it('resets the backoff once a reconnect gets its snapshot', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('sid');
		const { chat, ws } = await connected(fakeApi(start));
		ws.onclose?.();
		await vi.advanceTimersByTimeAsync(1500);
		const second = FakeSocket.last!;
		second.onclose?.(); // closed before a snapshot: the next wait doubles
		await vi.advanceTimersByTimeAsync(1500);
		expect(FakeSocket.last).toBe(second);
		await vi.advanceTimersByTimeAsync(1500);
		const third = FakeSocket.last!;
		expect(third).not.toBe(second);
		third.emit({
			t: 'snapshot',
			sessionId: 'sid',
			start: 0,
			items: [],
			meta: meta(),
			commands: [],
			exited: false
		});
		third.onclose?.();
		await vi.advanceTimersByTimeAsync(1500);
		expect(FakeSocket.last).not.toBe(third);
		chat.dispose();
	});

	it('shows an own chat whose session stopped while away as stopped, not retrying', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('sid');
		const { chat, ws } = await connected(fakeApi(start));
		start.mockRejectedValue(
			Object.assign(new Error('This chat ended on the other device.'), { status: 404 })
		);
		ws.onclose?.();
		// A relaunch unlists the session for up to 30s, so it's looked for a while first.
		await vi.advanceTimersByTimeAsync(1500);
		expect(chat.status).toBe('reconnecting');
		await vi.advanceTimersByTimeAsync(45_000);
		expect(chat.status).toBe('exited');
		expect(chat.error).toBeNull();
		const tries = start.mock.calls.length;
		expect(tries).toBe(6); // the start, then five looks
		await vi.advanceTimersByTimeAsync(120_000);
		expect(start).toHaveBeenCalledTimes(tries);
		chat.dispose();
	});

	it('rejoins a relaunched session whose replaced frame never arrived', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('sid');
		const { chat, ws } = await connected(fakeApi(start));
		start.mockRejectedValueOnce(Object.assign(new Error('gone'), { status: 404 }));
		ws.onclose?.();
		await vi.advanceTimersByTimeAsync(1500);
		expect(chat.status).toBe('reconnecting');
		await vi.advanceTimersByTimeAsync(3000);
		expect(FakeSocket.last).not.toBe(ws);
		expect(start).toHaveBeenLastCalledWith({ ...body, attachOnly: true });
		chat.dispose();
	});

	it('a joined chat refused on reconnect rejoins on Restart, never takes over', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('sid');
		const attach = { ...body, attachOnly: true };
		const { chat, ws } = await connected(fakeApi(start), attach);
		const onTakeOver = vi.fn();
		chat.onTakeOver = onTakeOver;
		start.mockRejectedValueOnce(Object.assign(new Error('unauthorized'), { status: 401 }));
		ws.onclose?.();
		await vi.advanceTimersByTimeAsync(1500);
		expect(chat.status).toBe('exited');
		await chat.open();
		expect(onTakeOver).not.toHaveBeenCalled();
		expect(start).toHaveBeenLastCalledWith(attach);
		chat.dispose();
	});

	it('gives up at once on a refusal that will not change, like a revoked token', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('sid');
		const { chat, ws } = await connected(fakeApi(start));
		start.mockRejectedValue(Object.assign(new Error('unauthorized'), { status: 401 }));
		ws.onclose?.();
		await vi.advanceTimersByTimeAsync(1500);
		expect(chat.status).toBe('exited');
		expect(chat.error).toBe('unauthorized');
		chat.dispose();
	});

	it('starts again on replaced, since the relaunched session is not listed yet', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('sid');
		const { chat, ws } = await connected(fakeApi(start));
		ws.emit({ t: 'replaced' });
		await vi.waitFor(() => expect(FakeSocket.last).not.toBe(ws));
		expect(start).toHaveBeenLastCalledWith(body);
		chat.dispose();
	});

	it('keeps retrying while the network is still down after a phone wakes', async () => {
		const start = vi
			.fn<AgentApi['start']>()
			.mockResolvedValueOnce('sid')
			.mockRejectedValueOnce(new Error('Failed to fetch'))
			.mockResolvedValue('sid');
		const { chat, ws } = await connected(fakeApi(start));
		chat.reconnect();
		await vi.waitFor(() => expect(start).toHaveBeenCalledTimes(2));
		expect(chat.status).toBe('reconnecting');

		await vi.advanceTimersByTimeAsync(1500);
		const retry = FakeSocket.last!;
		expect(retry).not.toBe(ws);
		retry.onclose?.(); // the socket couldn't open either: the next try waits twice as long
		expect(chat.status).toBe('reconnecting');
		await vi.advanceTimersByTimeAsync(1500);
		expect(start).toHaveBeenCalledTimes(3);
		await vi.advanceTimersByTimeAsync(1500);
		expect(start).toHaveBeenCalledTimes(4);
		expect(FakeSocket.last).not.toBe(retry);
		chat.dispose();
	});

	it('re-attaches at once on reconnect(), dropping the old socket quietly', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('sid');
		const { chat, ws } = await connected(fakeApi(start));
		chat.reconnect();
		expect(chat.status).toBe('reconnecting');
		expect(ws.readyState).toBe(3);
		await vi.waitFor(() => expect(FakeSocket.last).not.toBe(ws));
		expect(start).toHaveBeenCalledTimes(2);
		chat.dispose();
	});

	it('abandons a connect still waiting on the server when reconnect() starts another', async () => {
		let release!: (id: string) => void;
		const start = vi
			.fn<AgentApi['start']>()
			.mockResolvedValueOnce('sid')
			.mockImplementationOnce(() => new Promise((r) => (release = r)))
			.mockResolvedValue('sid');
		const { chat, ws } = await connected(fakeApi(start));
		ws.onclose?.(); // dropped: the retry timer fires and its start hangs
		await vi.advanceTimersByTimeAsync(1500);
		chat.reconnect();
		await vi.waitFor(() => expect(FakeSocket.last).not.toBe(ws));
		const current = FakeSocket.last;
		release('sid');
		await vi.advanceTimersByTimeAsync(0);
		expect(FakeSocket.last).toBe(current); // the stale connect opened no socket
		chat.dispose();
	});

	it('knows when there is history to resume, even beyond the snapshot', async () => {
		const { chat, ws } = await connected();
		expect(chat.hasHistory).toBe(false);
		ws.emit({
			t: 'snapshot',
			sessionId: 'sid',
			start: 500,
			items: [],
			meta: meta(),
			commands: [],
			exited: false
		});
		expect(chat.hasHistory).toBe(true);
		chat.dispose();
	});

	it('sends elicitation answers', async () => {
		const { chat, ws } = await connected();
		const item = {
			kind: 'elicitation',
			id: 'e1',
			server: 'deploy',
			message: 'Which environment?',
			mode: 'form',
			expired: false,
			completed: false
		} as const;
		ws.emit({ t: 'update', changes: [[0, item]], meta: meta(true) });
		chat.elicit('e1', 'accept', { env: 'prod' });
		chat.elicit('e2', 'decline');
		expect(ws.sent).toEqual([
			{ t: 'elicit', requestId: 'e1', action: 'accept', content: { env: 'prod' } },
			{ t: 'elicit', requestId: 'e2', action: 'decline' }
		]);
		chat.dispose();
	});

	it('closes the socket when the process ends', async () => {
		const { chat, ws } = await connected();
		ws.emit({ t: 'exit', code: 1, message: null });
		expect(chat.status).toBe('exited');
		expect(ws.readyState).toBe(3);
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

	it("fetches a background task's live output", async () => {
		const { chat, ws } = await connected();
		const out = chat.taskOutput('b1');
		expect(ws.sent).toEqual([{ t: 'taskOutput', taskId: 'b1' }]);
		ws.emit({ t: 'taskOutput', taskId: 'b1', text: 'compiling…', bytes: 40_000 });
		await expect(out).resolves.toEqual({ text: 'compiling…', bytes: 40_000 });
		chat.dispose();
	});

	it("reads a subagent's transcript for the session, null when it can't", async () => {
		const taskTranscript = vi
			.fn<NonNullable<AgentApi['taskTranscript']>>()
			.mockResolvedValueOnce({ start: 0, items: [] })
			.mockRejectedValueOnce(new Error('gone'));
		const { chat } = await connected({ ...fakeApi(), taskTranscript });
		await expect(chat.taskTranscript('toolu_1')).resolves.toEqual({ start: 0, items: [] });
		expect(taskTranscript).toHaveBeenCalledWith('sid', 'toolu_1');
		await expect(chat.taskTranscript('toolu_1')).resolves.toBeNull();
		chat.dispose();

		const bare = await connected();
		await expect(bare.chat.taskTranscript('toolu_1')).resolves.toBeNull();
		bare.chat.dispose();
	});

	it('keeps the slash command list up to date', async () => {
		const { chat, ws } = await connected();
		ws.emit({ t: 'commands', commands: [{ name: 'compact', description: 'Free up context' }] });
		expect(chat.commands.map((c) => c.name)).toEqual(['compact']);
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
	it('starts a new Codex thread and keeps its id for reconnects', async () => {
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('thread-1');
		const socketUrl = vi.fn(async (id: string) => `ws://test/agent/codex/${id}/ws`);
		const codex: StartAgentBody = { agent: 'codex', projectPath: '/repo' };
		const chat = new AgentChat(codex, { start, socketUrl });
		expect(chat.agent).toBe('codex');
		expect(chat.sessionId).toBe('');
		await vi.waitFor(() => expect(FakeSocket.last).not.toBeNull());
		expect(start).toHaveBeenCalledWith({ ...codex, sessionId: undefined });
		expect(chat.sessionId).toBe('thread-1');
		expect(socketUrl).toHaveBeenCalledWith('thread-1');

		const ws = FakeSocket.last!;
		ws.onclose?.();
		await vi.advanceTimersByTimeAsync(1500);
		expect(start).toHaveBeenLastCalledWith({ ...codex, sessionId: 'thread-1', attachOnly: true });
		chat.dispose();
	});

	it('a restart while a new Codex thread starts joins that start, not a second thread', async () => {
		let resolve!: (id: string) => void;
		const start = vi.fn<AgentApi['start']>((b) =>
			b.sessionId ? Promise.resolve(b.sessionId) : new Promise((r) => (resolve = r))
		);
		const socketUrl = vi.fn(async (id: string) => `ws://test/agent/codex/${id}/ws`);
		const chat = new AgentChat({ agent: 'codex', projectPath: '/repo' }, { start, socketUrl });
		await vi.waitFor(() => expect(start).toHaveBeenCalledTimes(1));
		void chat.open();
		resolve('thread-1');
		await vi.waitFor(() => expect(FakeSocket.last).not.toBeNull());
		expect(start).toHaveBeenCalledTimes(1);
		expect(chat.sessionId).toBe('thread-1');
		expect(socketUrl).toHaveBeenCalledWith('thread-1');
		chat.dispose();
	});

	it('names Codex when a Codex chat cannot send', async () => {
		const chat = new AgentChat({ agent: 'codex', projectPath: '/repo' }, fakeApi());
		await vi.waitFor(() => expect(FakeSocket.last).not.toBeNull());
		FakeSocket.last!.readyState = 3;
		expect(chat.prompt('hi')).toBe(false);
		expect(chat.notice).toBe('Not connected to Codex yet. Try again in a moment.');
		chat.dispose();
	});

	it('defaults to Claude', () => {
		const chat = new AgentChat(body, fakeApi());
		expect(chat.agent).toBe('claude');
		expect(chat.sessionId).toBe('sid');
		chat.dispose();
	});
	it('correlates native action replies across clients and settles timeouts', async () => {
		const { chat, ws } = await connected(fakeApi(), { ...body, agent: 'codex' });
		const first = chat.codexAction('compact');
		const second = chat.codexAction('inspect', { section: 'account' });
		const messages = ws.sent as { requestId: string }[];
		ws.emit({ t: 'codexResult', requestId: messages[1].requestId, result: { account: 'user' } });
		ws.emit({ t: 'codexResult', requestId: messages[0].requestId, result: {} });
		await expect(first).resolves.toEqual({});
		await expect(second).resolves.toEqual({ account: 'user' });
		const timeout = chat.codexAction('review');
		const rejected = expect(timeout).rejects.toThrow('timed out');
		await vi.advanceTimersByTimeAsync(35000);
		await rejected;
		chat.dispose();
	});
	it('disposal releases outstanding native actions', async () => {
		const { chat } = await connected();
		const action = chat.codexAction('inspect', { section: 'account' });
		const rejected = expect(action).rejects.toThrow('Chat closed');
		chat.dispose();
		await rejected;
	});
	it('queues shared file attachments and clears the draft only after acknowledgment', async () => {
		const { chat, ws } = await connected(fakeApi(), { ...body, agent: 'codex' });
		chat.receive({ t: 'update', changes: [], meta: meta(true) });
		chat.delivery = 'queue';
		const file = { name: 'README.md', mediaType: 'text/plain' as const, data: 'Hi' };
		const sending = chat.prompt('Use this file', [], [file]);
		const sent = ws.sent[0] as {
			t: string;
			action: string;
			params: { text: string; files: unknown[] };
			requestId: string;
		};
		expect(sent.t).toBe('codex');
		expect(sent.action).toBe('queueAdd');
		expect(sent.params).toMatchObject({ text: 'Use this file', files: [file] });
		ws.emit({ t: 'codexResult', requestId: sent.requestId, result: {} });
		await expect(sending).resolves.toBe(true);
		const retry = chat.prompt('', [], [file]);
		const failed = ws.sent.at(-1) as { requestId: string };
		ws.emit({ t: 'codexResult', requestId: failed.requestId, error: 'Queue full' });
		await expect(retry).resolves.toBe(false);
		expect(chat.notice).toBe('Queue full');
		chat.dispose();
	});
	it('older history follows a client cursor and resets on a different session', async () => {
		const { chat, ws } = await connected(fakeApi(), { ...body, agent: 'codex' });
		const first = chat.loadOlder();
		const request = ws.sent.at(-1) as { requestId: string };
		ws.emit({
			t: 'codexResult',
			requestId: request.requestId,
			result: { items: [{ kind: 'text', id: 'old', text: 'Earlier' }], nextCursor: 'next' }
		});
		await first;
		const second = chat.loadOlder();
		expect(ws.sent.at(-1)).toMatchObject({ params: { cursor: 'next' } });
		ws.emit({
			t: 'codexResult',
			requestId: (ws.sent.at(-1) as { requestId: string }).requestId,
			result: { items: [], nextCursor: null }
		});
		await second;
		expect(chat.hasOlderHistory).toBe(false);
		expect(chat.hasHistory).toBe(true);
		ws.emit({
			t: 'snapshot',
			sessionId: 'different',
			start: 0,
			items: [],
			commands: [],
			meta: meta(),
			exited: false
		});
		expect(chat.historyItems).toEqual([]);
		expect(chat.hasHistory).toBe(false);
		chat.dispose();
	});
	it('identical pending messages require separate echoes', async () => {
		const { chat, ws } = await connected();
		chat.prompt('same');
		chat.prompt('same');
		ws.emit({
			t: 'update',
			changes: [[0, { kind: 'user', id: 'a', text: 'same', timestamp: '' }]],
			meta: meta()
		});
		expect(chat.pending).toHaveLength(1);
		ws.emit({
			t: 'update',
			changes: [[1, { kind: 'text', id: 'text', text: 'Working' }]],
			meta: meta()
		});
		expect(chat.pending).toHaveLength(1);
		ws.emit({
			t: 'update',
			changes: [[2, { kind: 'user', id: 'b', text: 'same', timestamp: '' }]],
			meta: meta()
		});
		expect(chat.pending).toHaveLength(0);
		chat.dispose();
	});
});

describe('AgentChat prompt cache', () => {
	beforeEach(() => {
		vi.stubGlobal('WebSocket', FakeSocket);
		FakeSocket.last = null;
	});
	afterEach(() => vi.unstubAllGlobals());

	it('follows the server cache policy and sends pings and changes', async () => {
		const { chat, ws } = await connected();
		expect(chat.cachePolicy).toEqual({ compactOnExpiry: false });

		ws.emit({ t: 'cachePolicy', policy: { compactOnExpiry: true, keepWarmUntil: 5 } });
		expect(chat.cachePolicy).toEqual({ compactOnExpiry: true, keepWarmUntil: 5 });

		chat.pingCache();
		chat.setCachePolicy({ compactOnExpiry: false });
		expect(ws.sent).toEqual([
			{ t: 'cachePing' },
			{ t: 'cachePolicy', policy: { compactOnExpiry: false } }
		]);
		expect(chat.cachePolicy).toEqual({ compactOnExpiry: false });
	});
});

describe('AgentChat host lifecycle', () => {
	beforeEach(() => {
		vi.useFakeTimers();
		vi.stubGlobal('WebSocket', FakeSocket);
		FakeSocket.last = null;
	});
	afterEach(() => {
		vi.useRealTimers();
		vi.unstubAllGlobals();
	});

	it('keeps its draft for the life of the chat, or uses the one the host passes', async () => {
		const { chat } = await connected();
		chat.draft.text = 'half a thought';
		chat.draft.images = [{ name: 'a.png', mediaType: 'image/png', data: 'AA==' }];
		expect(chat.draft.text).toBe('half a thought');
		chat.dispose();

		const kept = new ChatDraft('saved');
		const other = new AgentChat(body, fakeApi(), { draft: kept });
		expect(other.draft).toBe(kept);
		other.dispose();
	});

	it('re-attaches after the page was hidden a while, and stops listening once disposed', async () => {
		const doc = Object.assign(new EventTarget(), { hidden: false });
		vi.stubGlobal('document', doc);
		const chat = new AgentChat(body, fakeApi(), { reconnectOnWake: true });
		await vi.waitFor(() => expect(FakeSocket.last).not.toBeNull());
		const ws = FakeSocket.last!;
		ws.emit({
			t: 'snapshot',
			sessionId: 'sid',
			start: 0,
			items: [],
			meta: meta(),
			commands: [],
			exited: false
		});
		const output = chat.fullOutput('t1');
		const wake = (hiddenFor: number) => {
			doc.hidden = true;
			doc.dispatchEvent(new Event('visibilitychange'));
			vi.advanceTimersByTime(hiddenFor);
			doc.hidden = false;
			doc.dispatchEvent(new Event('visibilitychange'));
		};
		wake(2_000);
		expect(chat.status).toBe('live');
		wake(60_000);
		expect(chat.status).toBe('reconnecting');
		await expect(output).resolves.toBeNull();
		await vi.waitFor(() => expect(FakeSocket.last).not.toBe(ws));

		const remove = vi.spyOn(doc, 'removeEventListener');
		chat.dispose();
		expect(remove).toHaveBeenCalledWith('visibilitychange', expect.any(Function));
	});

	it('retries on wake after running out of retries; a joined chat then rejoins, not takes over', async () => {
		const doc = Object.assign(new EventTarget(), { hidden: false });
		vi.stubGlobal('document', doc);
		const start = vi.fn<AgentApi['start']>().mockResolvedValue('sid');
		const attach = { ...body, attachOnly: true };
		const chat = new AgentChat(attach, fakeApi(start), { reconnectOnWake: true });
		const onTakeOver = vi.fn();
		chat.onTakeOver = onTakeOver;
		await vi.waitFor(() => expect(FakeSocket.last).not.toBeNull());
		const giveUp = async () => {
			start.mockRejectedValue(new Error('Failed to fetch'));
			await vi.advanceTimersByTimeAsync(200_000);
			expect(chat.status).toBe('exited');
			start.mockResolvedValue('sid');
		};
		doc.hidden = true;
		doc.dispatchEvent(new Event('visibilitychange'));
		FakeSocket.last!.onclose?.();
		await giveUp();

		const calls = start.mock.calls.length;
		const before = FakeSocket.last;
		doc.hidden = false;
		doc.dispatchEvent(new Event('visibilitychange'));
		expect(chat.status).toBe('reconnecting');
		await vi.waitFor(() => expect(FakeSocket.last).not.toBe(before));
		expect(start).toHaveBeenCalledTimes(calls + 1);
		expect(start).toHaveBeenLastCalledWith(attach);

		FakeSocket.last!.onclose?.();
		await giveUp();
		await chat.open();
		expect(onTakeOver).not.toHaveBeenCalled();
		expect(start).toHaveBeenLastCalledWith(attach);
		chat.dispose();
	});

	it('ends a joined chat that is gone on reconnect instead of retrying', async () => {
		const start = vi.fn<AgentApi['start']>(async (b) => b.sessionId ?? 'sid');
		const { chat } = await connected(fakeApi(start), { ...body, attachOnly: true });
		start.mockRejectedValue(Object.assign(new Error('ended'), { status: 404 }));
		chat.reconnect();
		await vi.waitFor(() => expect(chat.status).toBe('exited'));
		expect(chat.error).toBe('ended');
		chat.dispose();
	});

	it('counts sends, so the transcript can scroll back down', async () => {
		const { chat } = await connected();
		chat.prompt('hello');
		expect(chat.sends).toBe(1);
		chat.dispose();
	});

	it('names the agent when its session ends', async () => {
		const { chat, ws } = await connected();
		const action = chat.codexAction('history');
		ws.emit({ t: 'exit', code: 0, message: null });
		await expect(action).rejects.toThrow('The Claude session ended');
		chat.dispose();
	});
});
