import { beforeEach, expect, test } from 'bun:test';
import * as server from '../hooks/link';

// The host's fetch has been seen never to settle; these fake it doing so.

type Call = { url: string; body: Record<string, unknown> };
type Reply = { status: number; text: string } | 'hang' | 'throw';

let calls: Call[] = [];
let replies: Reply[] = [];

const fetch = ((url: string, init?: { body?: string }) => {
	calls.push({ url, body: JSON.parse(init?.body ?? '{}') });
	const reply = replies.shift() ?? { status: 204, text: '' };
	if (reply === 'hang') return new Promise(() => {});
	if (reply === 'throw') return Promise.reject(new Error('connection reset'));
	return Promise.resolve({ ...reply, ok: reply.status < 300, headers: {} });
}) as unknown as Parameters<typeof server.flush>[0];

/** Let pending promise chains run. */
const settle = async () => {
	for (let i = 0; i < 5; i++) await new Promise((resolve) => setTimeout(resolve, 0));
};

let now = 1_000_000;
server.clock.now = () => now;

/** Move the wall clock on and let the session's timer read it. */
async function advance(ms: number) {
	now += ms;
	server.tick();
	await settle();
}

/** Move the wall clock on without the timer running (it stopped). */
const passes = (ms: number) => (now += ms);

const answer = (behavior: string) => ({
	status: 200,
	text: JSON.stringify({ answer: { response: { subtype: 'success', response: { behavior } } } })
});
const pending = { status: 200, text: JSON.stringify({ pending: true }) };
const ok = { status: 204, text: '' };
const asked = { type: 'control_request', request_id: 'r1' };
const paths = () => calls.map((c) => new URL(c.url).pathname);
const noSignal = () => new AbortController().signal;

/** A fake `$.clock.after`: fires when `fire()` is called, if not cancelled. */
function fakeAfter() {
	const timers: { fn: () => void; cancelled: boolean }[] = [];
	const after: server.After = (_ms, fn) => {
		const t = { fn, cancelled: false };
		timers.push(t);
		return { cancel: () => (t.cancelled = true) };
	};
	const fire = () => timers.filter((t) => !t.cancelled).forEach((t) => t.fn());
	return { after, fire, timers };
}

beforeEach(async () => {
	// A previous test's hung request is given up, and its retry delay passed.
	await advance(60_000);
	await advance(2_000);
	server.open({ url: 'http://127.0.0.1:1', token: 't', sessionId: 's' });
	server.hello.needed = false;
	server.takeOutbox();
	calls = [];
	replies = [];
});

test('a hung post is given up and its lines go again with the next', async () => {
	replies = ['hang'];
	server.emit({ type: 'assistant', n: 1 });
	const first = server.flush(fetch);
	await settle();
	expect(server.isFlushing()).toBe(true);

	server.emit({ type: 'assistant', n: 2 });
	await advance(10_000);
	await first;
	expect(server.isFlushing()).toBe(false);

	await advance(1_000);
	await server.flush(fetch);
	const [hung, retry] = calls.map((c) => c.body);
	expect(retry.lines).toEqual([
		{ type: 'assistant', n: 1 },
		{ type: 'assistant', n: 2 }
	]);
	expect(retry.seq).toBe(hung.seq);

	calls = [];
	await server.flush(fetch);
	expect(calls).toEqual([]);
});

test('a confirmed post is not sent again, and the next continues the numbering', async () => {
	server.emit({ type: 'assistant', n: 1 }, { type: 'assistant', n: 2 });
	await server.flush(fetch);
	server.emit({ type: 'assistant', n: 3 });
	await server.flush(fetch);
	const [a, b] = calls.map((c) => c.body);
	expect(b.lines).toEqual([{ type: 'assistant', n: 3 }]);
	expect(b.seq).toBe((a.seq as number) + 2);
});

test('a failed post waits a moment before trying again', async () => {
	replies = ['throw'];
	server.emit({ type: 'assistant' });
	await server.flush(fetch);
	await server.flush(fetch);
	expect(calls.length).toBe(1);
	await advance(1_000);
	await server.flush(fetch);
	expect(calls.length).toBe(2);
});

test('a lost ask reply is asked again, resending the line until the server has it', async () => {
	replies = ['hang', pending, answer('allow')];
	const result = server.askInChat(fetch, undefined, 'r1', asked, noSignal(), true);
	await settle();
	await advance(30_000);
	expect(await result).toEqual({ behavior: 'allow' });
	expect(paths()).toEqual(['/mod/ask', '/mod/ask', '/mod/ask']);
	expect(calls.map((c) => c.body.line)).toEqual([asked, asked, undefined]);
});

test('an ask that keeps failing hands the card to the terminal directly', async () => {
	replies = ['throw', { status: 500, text: '' }, 'throw'];
	const result = await server.askInChat(fetch, undefined, 'r1', asked, noSignal(), true);
	await settle();
	expect(result).toBeNull();
	expect(paths()).toEqual(['/mod/ask', '/mod/ask', '/mod/ask', '/mod/out']);
	expect(calls[3].body.lines).toEqual([
		{ type: 'control_cancel_request', request_id: 'r1', workbench_in_terminal: true }
	]);
	expect(calls[3].body.seq).toBeUndefined();
});

test('a server fallback hands the question to the terminal without a withdraw', async () => {
	replies = [{ status: 200, text: JSON.stringify({ fallback: true }) }];
	const result = await server.askInChat(fetch, undefined, 'r1', asked, noSignal(), true);
	await settle();
	expect(result).toBeNull();
	expect(paths()).toEqual(['/mod/ask']);
});

test('the ask carries every unconfirmed line, those of a hung post too', async () => {
	replies = ['hang', answer('allow')];
	server.emit({ type: 'assistant', n: 1 });
	void server.flush(fetch);
	await settle();
	server.emit({ type: 'assistant', n: 2 });
	const result = server.askInChat(fetch, undefined, 'r1', asked, noSignal(), true);
	expect(await result).toEqual({ behavior: 'allow' });
	expect(paths()).toEqual(['/mod/out', '/mod/ask']);
	expect(calls[1].body.lines).toEqual([
		{ type: 'assistant', n: 1 },
		{ type: 'assistant', n: 2 }
	]);
	expect(calls[1].body.seq).toBe(calls[0].body.seq);

	// The ask's reply confirmed them: nothing is left to post.
	await advance(10_000);
	calls = [];
	await server.flush(fetch);
	expect(calls).toEqual([]);
});

test('Esc withdraws the card even while a post holds the queue', async () => {
	replies = ['hang', 'hang'];
	server.emit({ type: 'assistant' });
	void server.flush(fetch);
	await settle();
	const esc = new AbortController();
	const result = server.askInChat(fetch, undefined, 'r1', asked, esc.signal, true);
	await settle();
	esc.abort();
	expect(await result).toBeNull();
	await settle();
	expect(paths()).toEqual(['/mod/out', '/mod/ask', '/mod/out']);
	expect(calls[2].body.lines).toEqual([{ type: 'control_cancel_request', request_id: 'r1' }]);
});

test('lines a failed ask carried stay queued', async () => {
	replies = ['throw', 'throw', 'throw'];
	server.emit({ type: 'assistant', n: 1 });
	await server.askInChat(fetch, undefined, 'r1', asked, noSignal(), true);
	await settle();
	calls = [];
	await server.flush(fetch);
	expect(calls[0].body.lines).toEqual([{ type: 'assistant', n: 1 }]);
});

test("a bound fires on the wall clock when the session's timer has stopped", async () => {
	replies = ['hang'];
	server.emit({ type: 'assistant' });
	void server.flush(fetch);
	await settle();
	passes(10_000);
	await settle();
	expect(server.isFlushing()).toBe(true);
	// No tick: the next line queued checks the deadlines.
	server.emit({ type: 'assistant' });
	await settle();
	expect(server.isFlushing()).toBe(false);
});

test('an ask holds its bound through `$.clock.after` alone', async () => {
	const { after, fire } = fakeAfter();
	replies = ['hang', answer('deny')];
	const result = server.askInChat(fetch, after, 'r1', asked, noSignal(), true);
	await settle();
	fire();
	expect(await result).toEqual({ behavior: 'deny' });
});

test('a settled request cancels its timer', async () => {
	const { after, timers } = fakeAfter();
	replies = [answer('allow')];
	await server.askInChat(fetch, after, 'r1', asked, noSignal(), true);
	expect(timers.every((t) => t.cancelled)).toBe(true);
});

test('a hung poll is given up', async () => {
	replies = [{ status: 200, text: JSON.stringify({ inSeq: 0 }) }];
	await server.attach(fetch, false);
	replies = ['hang'];
	const lines = server.poll(fetch);
	await settle();
	await advance(30_000);
	expect(await lines).toEqual([]);
});

test('polled lines are handled once and acknowledged on the next poll', async () => {
	replies = [{ status: 200, text: JSON.stringify({ inSeq: 0 }) }];
	await server.attach(fetch, false);
	const line = (seq: number) => ({ type: 'user', wbSeq: seq });
	replies = [
		{ status: 200, text: JSON.stringify([line(1), line(2)]) },
		// The server never heard the ack: 2 comes again.
		{ status: 200, text: JSON.stringify([line(2), line(3)]) },
		{ status: 200, text: JSON.stringify([{ type: 'user' }]) }
	];
	const run = async () => {
		const got = await server.poll(fetch);
		got.forEach(server.handled);
		return got;
	};
	expect(await run()).toEqual([line(1), line(2)]);
	expect(await run()).toEqual([line(3)]);
	// An older server numbers nothing: every line counts.
	expect(await run()).toEqual([{ type: 'user' }]);
	const acks = calls.slice(1).map((c) => new URL(c.url).searchParams.get('ack'));
	expect(acks).toEqual(['0', '2', '3']);
});

test('a line not yet handled comes again', async () => {
	replies = [{ status: 200, text: JSON.stringify({ inSeq: 0 }) }];
	await server.attach(fetch, false);
	const line = { type: 'user', wbSeq: 1 };
	replies = [
		{ status: 200, text: JSON.stringify([line]) },
		{ status: 200, text: JSON.stringify([line]) }
	];
	expect(await server.poll(fetch)).toEqual([line]);
	expect(await server.poll(fetch)).toEqual([line]);
	expect(new URL(calls[2].url).searchParams.get('ack')).toBe('0');
});

test('every numbered request names this worker', async () => {
	replies = [ok, ok];
	await server.attach(fetch, false);
	server.emit({ type: 'assistant' });
	await server.flush(fetch);
	const [hello, out] = calls.map((c) => c.body);
	expect(typeof hello.epoch).toBe('string');
	expect(out.epoch).toBe(hello.epoch);
});

test('a hello the server answers from a session it still had keeps the queue', async () => {
	replies = ['hang'];
	const first = server.attach(fetch, false);
	await settle();
	await advance(5_000);
	expect(await first).toBe(false);
	server.emit({ type: 'user', n: 1 });
	replies = [{ status: 200, text: JSON.stringify({ inSeq: 0, loaded: false }) }];
	expect(await server.attach(fetch, true)).toBe(true);
	calls = [];
	await server.flush(fetch);
	expect(calls[0].body.lines).toEqual([{ type: 'user', n: 1 }]);
});

test('a session attached again keeps counting where the server stands', async () => {
	replies = [{ status: 200, text: JSON.stringify({ inSeq: 7 }) }];
	await server.attach(fetch, false);
	replies = [{ status: 200, text: JSON.stringify([{ type: 'user', wbSeq: 7 }]) }];
	expect(await server.poll(fetch)).toEqual([]);
	expect(new URL(calls[1].url).searchParams.get('ack')).toBe('7');
});

test('a failed hello keeps the link and is said again; queued lines go then', async () => {
	const { after, fire } = fakeAfter();
	replies = ['hang'];
	const first = server.attach(fetch, false, after);
	await settle();
	fire();
	expect(await first).toBe(false);
	expect(server.hello.needed).toBe(true);
	expect(server.current()).not.toBeNull();

	// Unattached: nothing is posted or polled.
	server.emit({ type: 'system', subtype: 'init' });
	calls = [];
	await server.flush(fetch);
	expect(await server.poll(fetch)).toEqual([]);
	expect(calls).toEqual([]);

	// The server loads the session's history: what was queued meanwhile goes.
	replies = [{ status: 204, text: '' }];
	expect(await server.attach(fetch, true)).toBe(true);
	expect(server.hello.needed).toBe(false);
	await server.flush(fetch);
	expect(paths()).toEqual(['/mod/hello']);

	server.emit({ type: 'assistant' });
	await server.flush(fetch);
	expect(paths()).toEqual(['/mod/hello', '/mod/out']);
});

test('the first hello keeps lines queued before it', async () => {
	server.emit({ type: 'system', subtype: 'init' });
	replies = [ok];
	expect(await server.attach(fetch, false)).toBe(true);
	await server.flush(fetch);
	expect(calls[1].body.lines).toEqual([{ type: 'system', subtype: 'init' }]);
});

test('a 404 marks the session lost', async () => {
	replies = [{ status: 404, text: '' }];
	server.emit({ type: 'assistant' });
	await server.flush(fetch);
	expect(server.hello.needed).toBe(true);
});
