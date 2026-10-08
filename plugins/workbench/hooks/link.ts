import type { EngineInterface, Timer } from 'claude-code';
import { askAnswer, type Answer, type Line } from './lines';

// The session's link to the Workbench server: lines go out through a queue
// (`/mod/out`), approvals are held open (`/mod/ask`), and the server's lines
// are long-polled (`/mod/in`). The hooks module hands in the engine calls it
// needs, spelled there.
//
// Both directions are numbered so a lost reply costs nothing: every line this
// side sends carries its number (`seq`, the first line's, the rest following)
// and stays queued until a reply confirms it, so each request resends all the
// unconfirmed ones from the oldest and the server folds a number once; the
// server keeps each `/mod/in` line until a later poll acknowledges it (`ack`).
// The numbering is this worker's (`epoch`): a restarted worker (a hot reload)
// starts again at 1 and the server counts afresh from its hello.

type Fetch = EngineInterface['http']['fetch'];
/** `$.clock.after`, spelled in the hook: a bound that holds without the session's timer. */
export type After = (ms: number, fn: () => void) => Timer;

// The host's fetch takes no signal and has been seen never to settle, so every
// request is given up after a bound: a hung post must not hold the queue (and
// the whole chat) forever. The server holds `/mod/ask` and `/mod/in` up to 20 s.
const POST_MS = 10_000;
const ASK_MS = 30_000;
const POLL_MS = 30_000;
export const HELLO_MS = 5_000;
// Failed `/mod/ask` calls in a row before the terminal's dialog takes over.
const ASK_TRIES = 3;
// A failed post is retried after this, not on every tick with a growing body.
const RETRY_MS = 1_000;
// Lines kept while the server can't take them (it's down); the oldest go.
const OUTBOX_KEPT = 5_000;
/** Between hellos while the server doesn't know the session. */
export const HELLO_EVERY_MS = 5_000;

interface Link {
	url: string;
	token: string;
	sessionId: string;
}

let link: Link | null = null;
const epoch = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
let outbox: { seq: number; line: Line }[] = [];
let lastSeq = 0;
// The newest `/mod/in` line handled, acknowledged on the next poll.
let inSeq = 0;
let flushing: Promise<void> | null = null;
let retryAt = 0;
// `/clear` or `/resume` ended the session and the process carries on under another id.
let rekeyPending: 'clear' | 'resume' | undefined;

/** The server doesn't know this session (it restarted, a stop detached it, the first hello failed). */
export const hello = { needed: false, last: 0 };

/** The time deadlines are read against; tests move it. */
export const clock = { now: () => Date.now() };

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

// Queued, then posted in order (see the top).
export function emit(...lines: Line[]) {
	if (!link) return;
	for (const line of lines) outbox.push({ seq: ++lastSeq, line });
	if (outbox.length > OUTBOX_KEPT) outbox.splice(0, outbox.length - OUTBOX_KEPT);
	tick();
}

type Batch = { seq?: number; lines: Line[] };

/** Every unconfirmed line, as a request carries them. */
function unconfirmed(): Batch & { epoch: string } {
	return { epoch, seq: outbox[0]?.seq, lines: outbox.map((e) => e.line) };
}

/** A reply showed the server has `batch`. */
function confirm(batch: Batch) {
	if (batch.seq === undefined) return;
	const last = batch.seq + batch.lines.length - 1;
	outbox = outbox.filter((e) => e.seq > last);
}

/** Everything queued, for the last request a session sends (`/mod/bye`). */
export function takeOutbox() {
	const batch = unconfirmed();
	outbox = [];
	return { sessionId: link?.sessionId, ...batch };
}

export function isFlushing() {
	return flushing !== null;
}

// Deadlines read the wall clock: `Date.now()` advances in the hooks
// environment, unlike a `$.clock.sleep`, which would spend the waiting hook's
// budget (an approval may be held for minutes). They're checked on the
// session's timer, on every line queued and whenever a bounded request
// settles; one that must hold even if the timer stops also gets an `After`.
const deadlines = new Set<{ at: number; done: () => void }>();

export function tick() {
	const now = clock.now();
	for (const d of deadlines) if (d.at <= now) d.done();
}

/** `work`, or `undefined` once `ms` have passed. */
export function within<T>(ms: number, work: Promise<T>, after?: After): Promise<T | undefined> {
	let done = () => {};
	const timeout = new Promise<undefined>((resolve) => (done = () => resolve(undefined)));
	const deadline = { at: clock.now() + ms, done };
	deadlines.add(deadline);
	let timer: Timer | undefined;
	try {
		timer = after?.(ms, done);
	} catch {
		// The tick still bounds it.
	}
	return Promise.race([work, timeout]).finally(() => {
		deadlines.delete(deadline);
		timer?.cancel();
		tick();
	});
}

/** Post what is queued, after any post in flight. */
export async function flush(fetch: Fetch) {
	while (flushing) await flushing;
	if (outbox.length === 0 || !link || hello.needed || clock.now() < retryAt) return;
	const batch = unconfirmed();
	const post = fetch(`${link.url}/mod/out`, init('POST', { sessionId: link.sessionId, ...batch }));
	flushing = within(POST_MS, post)
		.then((res) => {
			if (res?.status === 404) hello.needed = true;
			if (res?.ok) confirm(batch);
			else retryAt = clock.now() + RETRY_MS;
		})
		.catch(() => {
			retryAt = clock.now() + RETRY_MS;
		})
		.finally(() => (flushing = null));
	await flushing;
}

/**
 * Attach the session (`/mod/hello`). When the server loaded it afresh (it had
 * lost it), the history it read holds what was queued meanwhile: those lines
 * go. A session it still had (an earlier hello's reply was lost) keeps them.
 */
export async function attach(fetch: Fetch, again: boolean, after?: After): Promise<boolean> {
	if (!link) return false;
	const queued = lastSeq;
	hello.last = clock.now();
	const res = await within(
		HELLO_MS,
		fetch(`${link.url}/mod/hello`, init('POST', { sessionId: link.sessionId, epoch })).catch(
			() => null
		),
		after
	);
	if (!res?.ok) {
		hello.needed = true;
		return false;
	}
	hello.needed = false;
	retryAt = 0;
	const reply = helloReply(res.text);
	// Where the server's lines stand: a new link starts at 0; one it still had
	// counts from the newest this worker (or the one before it) acknowledged.
	inSeq = reply.loaded ? reply.inSeq : Math.max(inSeq, reply.inSeq);
	if (again && reply.loaded) outbox = outbox.filter((e) => e.seq > queued);
	return true;
}

/** An older server answers nothing: every hello loaded the session. */
function helloReply(text: string): { inSeq: number; loaded: boolean } {
	try {
		const reply = JSON.parse(text) as { inSeq?: unknown; loaded?: unknown };
		return {
			inSeq: typeof reply.inSeq === 'number' ? reply.inSeq : 0,
			loaded: reply.loaded !== false
		};
	} catch {
		return { inSeq: 0, loaded: true };
	}
}

/**
 * The server's lines for this session (a chat's prompts and requests) not
 * yet handled; each is acknowledged with `handled` once it has run.
 */
export async function poll(fetch: Fetch): Promise<Line[]> {
	if (!link || hello.needed) return [];
	const res = await within(
		POLL_MS,
		fetch(`${link.url}/mod/in?${sessionQuery()}&ack=${inSeq}`, init('GET')).catch(() => null)
	);
	if (res?.status === 404) hello.needed = true;
	if (!res?.ok) return [];
	const lines = JSON.parse(res.text || '[]') as Line[];
	// An older server numbers nothing.
	return lines.filter((line) => typeof line.wbSeq !== 'number' || line.wbSeq > inSeq);
}

/** `line` from `poll` has run (or failed): the next poll acknowledges it. */
export function handled(line: Line) {
	if (typeof line.wbSeq === 'number') inSeq = Math.max(inSeq, line.wbSeq);
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
	after: After | undefined,
	requestId: string,
	line: Line,
	signal: AbortSignal,
	hold: boolean
): Promise<Answer | null> {
	// The tool's card reaches the chat before its approval does: the queued
	// lines go with the request, as waiting on the queue's post, a `$` call of
	// another context, would spend this hook's budget.
	// Sent until a reply shows the server has it (it ignores a repeat).
	let first: Line | undefined = line;
	let answer: Answer | null | undefined;
	let failed = 0;
	while (answer === undefined && link && !signal.aborted) {
		const batch = unconfirmed();
		const res = await untilAborted(
			signal,
			within(
				ASK_MS,
				fetch(
					`${link.url}/mod/ask`,
					init('POST', { sessionId: link.sessionId, requestId, line: first, ...batch, hold })
				).catch(() => null),
				after
			)
		);
		if (signal.aborted) break;
		// A lost reply is asked again: the server keeps an answer until its call has a result.
		if (!res?.ok) {
			if (res?.status === 404) hello.needed = true;
			if (++failed < ASK_TRIES) continue;
			// The chat's card would stay answerable with nobody waiting on it.
			withdraw(fetch, requestId);
			return null;
		}
		confirm(batch);
		failed = 0;
		first = undefined;
		answer = askAnswer(res.text);
	}
	if (signal.aborted) withdraw(fetch, requestId);
	return answer ?? null;
}

/**
 * A request only the terminal's dialog can answer: the server shows the
 * session waiting on it there (and says so to phone and desktop) without a
 * card. The queued lines go first, as with `askInChat`.
 */
export async function askInTerminal(
	fetch: Fetch,
	after: After | undefined,
	requestId: string,
	line: Line
) {
	if (!link) return;
	const batch = unconfirmed();
	const res = await within(
		POST_MS,
		fetch(
			`${link.url}/mod/ask`,
			init('POST', { sessionId: link.sessionId, requestId, line, ...batch, terminal: true })
		).catch(() => null),
		after
	);
	if (res?.ok) confirm(batch);
}
