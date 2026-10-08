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
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

let now = 0;

/** Move the engine's clock on, as the plugin's timer reads it. */
async function advance(ms: number) {
	now += ms;
	server.tick(now);
	await settle();
	await settle();
}

const answer = (behavior: string) => ({
	status: 200,
	text: JSON.stringify({ answer: { response: { subtype: 'success', response: { behavior } } } })
});
const pending = { status: 200, text: JSON.stringify({ pending: true }) };
const asked = { type: 'control_request', request_id: 'r1' };
const paths = () => calls.map((c) => new URL(c.url).pathname);

beforeEach(async () => {
	// A previous test's hung post is given up before this one starts.
	await advance(30_000);
	server.open({ url: 'http://127.0.0.1:1', token: 't', sessionId: 's' });
	server.takeOutbox();
	calls = [];
	replies = [];
});

test('a hung post is given up and the queue moves on', async () => {
	replies = ['hang'];
	server.emit({ type: 'assistant', n: 1 });
	const first = server.flush(fetch);
	await settle();
	expect(server.isFlushing()).toBe(true);

	server.emit({ type: 'assistant', n: 2 });
	await advance(10_000);
	await first;
	expect(server.isFlushing()).toBe(false);

	await server.flush(fetch);
	expect(calls.map((c) => c.body.lines)).toEqual([
		[{ type: 'assistant', n: 1 }],
		[{ type: 'assistant', n: 2 }]
	]);
});

test('a lost ask reply is asked again, resending the line until the server has it', async () => {
	replies = ['hang', pending, answer('allow')];
	const result = server.askInChat(fetch, 'r1', asked, new AbortController().signal, true);
	await settle();
	await advance(30_000);
	expect(await result).toEqual({ behavior: 'allow' });
	expect(paths()).toEqual(['/mod/ask', '/mod/ask', '/mod/ask']);
	expect(calls.map((c) => c.body.line)).toEqual([asked, asked, undefined]);
});

test('an ask that keeps failing falls back and withdraws the card directly', async () => {
	replies = ['throw', { status: 500, text: '' }, 'throw'];
	const result = await server.askInChat(fetch, 'r1', asked, new AbortController().signal, true);
	await settle();
	expect(result).toBeNull();
	expect(paths()).toEqual(['/mod/ask', '/mod/ask', '/mod/ask', '/mod/out']);
	expect(calls[3].body.lines).toEqual([{ type: 'control_cancel_request', request_id: 'r1' }]);
});

test('a server fallback hands the question to the terminal without a withdraw', async () => {
	replies = [{ status: 200, text: JSON.stringify({ fallback: true }) }];
	const result = await server.askInChat(fetch, 'r1', asked, new AbortController().signal, true);
	await settle();
	expect(result).toBeNull();
	expect(paths()).toEqual(['/mod/ask']);
});

test('the ask carries the queued lines instead of waiting on a hung post', async () => {
	replies = ['hang', answer('allow')];
	server.emit({ type: 'assistant', n: 1 });
	void server.flush(fetch);
	await settle();
	server.emit({ type: 'assistant', n: 2 });
	const result = server.askInChat(fetch, 'r1', asked, new AbortController().signal, true);
	expect(await result).toEqual({ behavior: 'allow' });
	expect(paths()).toEqual(['/mod/out', '/mod/ask']);
	expect(calls[1].body.lines).toEqual([{ type: 'assistant', n: 2 }]);
});

test('Esc withdraws the card even while a post holds the queue', async () => {
	replies = ['hang', 'hang'];
	server.emit({ type: 'assistant' });
	void server.flush(fetch);
	await settle();
	const esc = new AbortController();
	const result = server.askInChat(fetch, 'r1', asked, esc.signal, true);
	await settle();
	esc.abort();
	expect(await result).toBeNull();
	await settle();
	expect(paths()).toEqual(['/mod/out', '/mod/ask', '/mod/out']);
	expect(calls[2].body.lines).toEqual([{ type: 'control_cancel_request', request_id: 'r1' }]);
});
