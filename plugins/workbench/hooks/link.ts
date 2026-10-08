import type { EngineInterface } from 'claude-code';
import { askAnswer, type Answer, type Line } from './lines';

// The session's link to the Workbench server: lines go out through a queue
// (`/mod/out`), approvals are held open (`/mod/ask`). The hooks module hands
// in the engine calls it needs, spelled there.

type Fetch = EngineInterface['http']['fetch'];

// The host's fetch takes no signal and has been seen never to settle, so every
// request is given up after a bound: a hung post must not hold the queue (and
// the whole chat) forever. The server holds `/mod/ask` up to 20 s.
const POST_MS = 10_000;
const ASK_MS = 30_000;
// Failed `/mod/ask` calls in a row before the terminal's dialog takes over.
const ASK_TRIES = 3;

interface Link {
	url: string;
	token: string;
	sessionId: string;
}

let link: Link | null = null;
let outbox: Line[] = [];
let flushing: Promise<void> | null = null;
// `/clear` or `/resume` ended the session and the process carries on under another id.
let rekeyPending: 'clear' | 'resume' | undefined;

/** The server lost this session (it restarted, or a stop detached it): say hello again. */
export const hello = { needed: false, last: 0 };

export function current(): Link | null {
	return link;
}

export function open(next: Link | null) {
	link = next;
}

export function init(method: string, body?: unknown) {
	return {
		method,
		headers: { 'content-type': 'application/json', 'x-workbench-mod-token': link?.token ?? '' },
		body: body === undefined ? undefined : JSON.stringify(body)
	};
}

export function sessionQuery() {
	return `sessionId=${encodeURIComponent(link?.sessionId ?? '')}`;
}

// Queued, then posted one request at a time, so lines reach the server in
// the order they happened.
export function emit(...lines: Line[]) {
	if (link) outbox.push(...lines);
}

export function takeOutbox() {
	const lines = outbox;
	outbox = [];
	return { sessionId: link?.sessionId, lines };
}

export function isFlushing() {
	return flushing !== null;
}

// Deadlines checked on the session's timer (`tick`, with `$.clock.now()`): a
// `$.clock.sleep` would spend the waiting hook's budget, and an approval may
// be held for minutes. One set before the first reading starts at it.
let now: number | undefined;
const deadlines = new Set<{ at?: number; ms: number; done: () => void }>();

export function tick(time: number) {
	now = time;
	for (const d of deadlines) {
		d.at ??= time + d.ms;
		if (d.at <= time) d.done();
	}
}

/** `work`, or `undefined` once `ms` have passed. */
function within<T>(ms: number, work: Promise<T>): Promise<T | undefined> {
	const deadline = { at: now === undefined ? undefined : now + ms, ms, done: () => {} };
	const timeout = new Promise<undefined>((resolve) => (deadline.done = () => resolve(undefined)));
	deadlines.add(deadline);
	return Promise.race([work, timeout]).finally(() => deadlines.delete(deadline));
}

/** Post what is queued, after any post in flight. */
export async function flush(fetch: Fetch) {
	while (flushing) await flushing;
	if (outbox.length === 0 || !link) return;
	const post = fetch(`${link.url}/mod/out`, init('POST', takeOutbox())).then((res) => {
		if (res.status === 404) hello.needed = true;
	});
	flushing = within(POST_MS, post)
		.then(() => {})
		.catch(() => {})
		.finally(() => (flushing = null));
	await flushing;
}

/** Withdraw a chat card at once, past a queue a hung post may hold. */
function withdraw(fetch: Fetch, requestId: string) {
	if (!link) return;
	const lines = [{ type: 'control_cancel_request', request_id: requestId }];
	void within(
		POST_MS,
		fetch(`${link.url}/mod/out`, init('POST', { sessionId: link.sessionId, lines }))
	).catch(() => {});
}

/** The session ended (`clear`, `resume`) and the process goes on under another id. */
export function expectRekey(reason: 'clear' | 'resume') {
	rekeyPending = reason;
}

/** After `/clear` or `/resume` the server hears of the new id before any of its lines. */
export async function rekey(sessionId: () => Promise<string>) {
	if (!rekeyPending || !link) return;
	const id = await sessionId();
	if (!rekeyPending || !link || id === link.sessionId) return;
	const resumed = rekeyPending === 'resume';
	rekeyPending = undefined;
	// The server finds the session by token until the reset re-keys it.
	emit({
		type: 'conversation_reset',
		new_conversation_id: id,
		session_id: link.sessionId,
		...(resumed ? { resumed } : {})
	});
	link = { ...link, sessionId: id };
}

/** `work`, or `undefined` as soon as `signal` aborts. */
function untilAborted<T>(signal: AbortSignal, work: Promise<T>): Promise<T | undefined> {
	return Promise.race([
		work,
		new Promise<undefined>((resolve) =>
			signal.addEventListener('abort', () => resolve(undefined), { once: true })
		)
	]);
}

/**
 * Ask in chat and hold until a client answers: `null` falls back to the
 * terminal's dialog. `hold` keeps it a chat's even before one has it open.
 * An abort (Esc) withdraws the card at once.
 */
export async function askInChat(
	fetch: Fetch,
	requestId: string,
	line: Line,
	signal: AbortSignal,
	hold: boolean
): Promise<Answer | null> {
	// The tool's card reaches the chat before its approval does. The queued
	// lines go with the request: waiting on the queue's post, a `$` call of
	// another context, would spend this hook's budget.
	let lines: Line[] | undefined = takeOutbox().lines;
	// Sent until a reply shows the server has it (it ignores a repeat).
	let first: Line | undefined = line;
	let answer: Answer | null | undefined;
	let failed = 0;
	while (answer === undefined && link && !signal.aborted) {
		const res = await untilAborted(
			signal,
			within(
				ASK_MS,
				fetch(
					`${link.url}/mod/ask`,
					init('POST', { sessionId: link.sessionId, requestId, line: first, lines, hold })
				).catch(() => null)
			)
		);
		if (signal.aborted) break;
		// A lost reply is asked again: the server keeps an answer until its call has a result.
		if (!res?.ok) {
			if (++failed < ASK_TRIES) continue;
			// The chat's card would stay answerable with nobody waiting on it.
			requeue(lines);
			withdraw(fetch, requestId);
			return null;
		}
		failed = 0;
		first = undefined;
		lines = undefined;
		answer = askAnswer(res.text);
	}
	if (signal.aborted) {
		requeue(lines);
		withdraw(fetch, requestId);
	}
	return answer ?? null;
}

/** Lines a request took that the server never confirmed: back to the front of the queue. */
function requeue(lines: Line[] | undefined) {
	if (lines?.length && link) outbox = [...lines, ...outbox];
}

/**
 * A request only the terminal's dialog can answer: the server shows the
 * session waiting on it there (and says so to phone and desktop) without a
 * card. The queued lines go first, as with `askInChat`.
 */
export async function askInTerminal(fetch: Fetch, requestId: string, line: Line) {
	if (!link) return;
	const lines = takeOutbox().lines;
	const res = await within(
		POST_MS,
		fetch(
			`${link.url}/mod/ask`,
			init('POST', { sessionId: link.sessionId, requestId, line, lines, terminal: true })
		).catch(() => null)
	);
	if (!res?.ok) requeue(lines);
}
