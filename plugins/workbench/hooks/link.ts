import type { EngineInterface } from 'claude-code';
import { askAnswer, type Answer, type Line } from './lines';

// The session's link to the Workbench server: lines go out through a queue
// (`/mod/out`), approvals are held open (`/mod/ask`). The hooks module hands
// in the engine calls it needs, spelled there.

type Fetch = EngineInterface['http']['fetch'];

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

/** Post what is queued, after any post in flight. */
export async function flush(fetch: Fetch) {
	while (flushing) await flushing;
	if (outbox.length === 0 || !link) return;
	flushing = fetch(`${link.url}/mod/out`, init('POST', takeOutbox()))
		.then((res) => {
			if (res.status === 404) hello.needed = true;
		})
		.catch(() => {})
		.finally(() => (flushing = null));
	await flushing;
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
 * terminal's dialog. An abort (Esc) withdraws the card at once.
 */
export async function askInChat(
	fetch: Fetch,
	requestId: string,
	line: Line,
	signal: AbortSignal
): Promise<Answer | null> {
	// The tool's card reaches the chat before its approval does.
	await flush(fetch);
	let first: Line | undefined = line;
	let answer: Answer | null | undefined;
	while (answer === undefined && link && !signal.aborted) {
		const res = await untilAborted(
			signal,
			fetch(
				`${link.url}/mod/ask`,
				init('POST', { sessionId: link.sessionId, requestId, line: first })
			).catch(() => null)
		);
		first = undefined;
		if (!signal.aborted) answer = askAnswer(res?.ok ? res.text : undefined);
	}
	if (signal.aborted) emit({ type: 'control_cancel_request', request_id: requestId });
	return answer ?? null;
}
