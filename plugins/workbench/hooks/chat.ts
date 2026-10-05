import type { EngineInterface, Register } from 'claude-code';

// Runs this interactive `claude` as a Workbench chat too: what `claude -p`
// would print as stream-json is posted to the server (`/mod/out`), and what it
// would read is long-polled (`/mod/in`), so the chat view and the terminal are
// one process. Workbench sets both variables on terminal shells it starts;
// outside Workbench the module does nothing.

type Line = Record<string, unknown>;
type Answer = { behavior?: string; message?: string };

interface Link {
	url: string;
	token: string;
	sessionId: string;
}

let link: Link | null = null;
let outbox: Line[] = [];
let flushing = false;
let polling = false;
let model = '';
let messageSeq = 0;
let currentMessage = '';
const startedBlocks = new Set<number>();
let askSeq = 0;
const pendingAsks = new Map<string, (answer: Answer) => void>();

// Questions and plans are answered by editing the tool's input, which a
// permission decision can't carry: the TUI keeps those.
const TUI_ONLY = new Set(['AskUserQuestion', 'ExitPlanMode']);

function init(method: string, body?: unknown) {
	return {
		method,
		headers: { 'content-type': 'application/json', 'x-workbench-mod-token': link?.token ?? '' },
		body: body === undefined ? undefined : JSON.stringify(body)
	};
}

function sessionQuery() {
	return `sessionId=${encodeURIComponent(link?.sessionId ?? '')}`;
}

// Queued, then posted by the flush timer one request at a time, so lines
// reach the server in the order they happened.
function emit(...lines: Line[]) {
	if (link) outbox.push(...lines);
}

function takeOutbox() {
	const lines = outbox;
	outbox = [];
	return { sessionId: link?.sessionId, lines };
}

function reply(requestId: unknown, error?: string) {
	emit({
		type: 'control_response',
		response: error
			? { subtype: 'error', request_id: requestId, error }
			: { subtype: 'success', request_id: requestId, response: {} }
	});
}

function promptText(content: unknown): { text: string; hasFiles: boolean } {
	if (typeof content === 'string') return { text: content, hasFiles: false };
	const blocks = Array.isArray(content) ? (content as { type?: string; text?: string }[]) : [];
	return {
		text: blocks
			.filter((b) => b.type === 'text')
			.map((b) => b.text ?? '')
			.join('\n'),
		hasFiles: blocks.some((b) => b.type === 'image' || b.type === 'document')
	};
}

function settleAsk(line: Line) {
	const response = line.response as
		| { request_id?: string; response?: Answer; error?: string }
		| undefined;
	const id = response?.request_id;
	const settle = id ? pendingAsks.get(id) : undefined;
	if (!id || !settle) return;
	pendingAsks.delete(id);
	settle(response?.response ?? { behavior: 'deny', message: response?.error });
}

export const register: Register = (on) => {
	on('session.start', async ($, e, next) => {
		const result = await next(e);
		const url = await $.env.get('WORKBENCH_MOD_URL');
		const token = await $.env.get('WORKBENCH_MOD_TOKEN');
		if (!url || !token) return result;
		const sessionId = await $.session.id();
		model = await $.session.model();
		link = { url, token, sessionId };
		const hello = await $.http
			.fetch(`${url}/mod/hello`, init('POST', { sessionId }))
			.catch(() => null);
		if (!hello?.ok) {
			link = null;
			return result;
		}
		emit({ type: 'system', subtype: 'init', session_id: sessionId, model });

		$.clock.every(50, () => {
			if (flushing || outbox.length === 0 || !link) return;
			flushing = true;
			$.http
				.fetch(`${link.url}/mod/out`, init('POST', takeOutbox()))
				.catch(() => {})
				.finally(() => (flushing = false));
		});

		$.clock.every(300, () => {
			if (polling || !link) return;
			polling = true;
			$.http
				.fetch(`${link.url}/mod/in?${sessionQuery()}`, init('GET'))
				.then(async (res) => {
					if (!res.ok) return;
					for (const line of JSON.parse(res.text || '[]') as Line[]) {
						if (line.type === 'control_response') {
							settleAsk(line);
						} else if (line.type === 'user') {
							const message = line.message as { content?: unknown } | undefined;
							const { text, hasFiles } = promptText(message?.content);
							if (hasFiles) {
								emit({
									type: 'system',
									subtype: 'local_command_output',
									content: 'Images and files can only be sent from the terminal for now.',
									uuid: `wbmod-note-${++askSeq}`
								});
							}
							if (text) await $.prompt.submit({ text, asUser: true });
						} else if (line.type === 'control_request') {
							const sub = (line.request as { subtype?: string } | undefined)?.subtype;
							if (sub === 'interrupt') {
								await $.session.abort({});
								reply(line.request_id);
							} else if (sub === 'initialize') {
								reply(line.request_id);
							} else {
								reply(
									line.request_id,
									`${sub ?? 'This'} isn't available in a terminal session yet.`
								);
							}
						}
					}
				})
				.catch(() => {})
				.finally(() => (polling = false));
		});
		return result;
	});

	on('session.end', async ($, e, next) => {
		if (link) {
			const { url } = link;
			if (outbox.length > 0)
				await $.http.fetch(`${url}/mod/out`, init('POST', takeOutbox())).catch(() => {});
			await $.http
				.fetch(`${url}/mod/bye`, init('POST', { sessionId: link.sessionId }))
				.catch(() => {});
			link = null;
		}
		return next(e);
	});

	on('turn.step', async function* ($, e, next) {
		const stream = next(e);
		if (!link || e.agentId) return yield* stream;
		currentMessage = `wbmod-${link.sessionId.slice(0, 8)}-${++messageSeq}`;
		startedBlocks.clear();
		model = e.model || model;
		emit({
			type: 'stream_event',
			event: { type: 'message_start', message: { id: currentMessage, model } }
		});
		for await (const chunk of stream) {
			if (chunk.kind === 'text' || chunk.kind === 'thinking') {
				if (!startedBlocks.has(chunk.index)) {
					startedBlocks.add(chunk.index);
					emit({
						type: 'stream_event',
						event: {
							type: 'content_block_start',
							index: chunk.index,
							content_block: { type: chunk.kind, text: '' }
						}
					});
				}
				emit({
					type: 'stream_event',
					event: {
						type: 'content_block_delta',
						index: chunk.index,
						delta:
							chunk.kind === 'text'
								? { type: 'text_delta', text: chunk.text }
								: { type: 'thinking_delta', thinking: chunk.text }
					}
				});
			} else if (chunk.kind === 'tool') {
				startedBlocks.add(chunk.index);
				emit({
					type: 'stream_event',
					event: {
						type: 'content_block_start',
						index: chunk.index,
						content_block: { type: 'tool_use', id: chunk.id, name: chunk.name, input: {} }
					}
				});
			}
			yield chunk;
		}
		return stream.result;
	});

	on('session.append', ($, e, next) => {
		const m = e.message;
		if (link && !e.agentId && !m.isMeta) {
			if (m.type === 'assistant' && e.door === 'response') {
				emit({
					type: 'assistant',
					uuid: e.uuid,
					session_id: link.sessionId,
					message: { ...m, id: currentMessage || `wbmod-${e.uuid}`, model }
				});
			} else if (m.type === 'user' && (e.door === 'prompt' || e.door === 'tool-result')) {
				emit({ type: 'user', uuid: e.uuid, session_id: link.sessionId, message: m });
			}
		}
		return next(e);
	});

	on('turn.complete', ($, e, next) => {
		if (link && !('agentId' in e && e.agentId)) {
			emit({ type: 'result', subtype: 'success', is_error: e.reason === 'error' });
		}
		return next(e);
	});

	on('tool.check', async ($, e, next) => {
		const verdict = await next(e);
		if (verdict.decision !== 'ask' || !link || TUI_ONLY.has(e.tool)) return verdict;
		const route = await $.http
			.fetch(`${link.url}/mod/route?${sessionQuery()}`, init('GET'))
			.catch(() => null);
		if (!route?.ok || !(JSON.parse(route.text) as { chat?: boolean }).chat) return verdict;
		const requestId = `wbmod-ask-${++askSeq}`;
		const answer = new Promise<Answer>((resolve) => pendingAsks.set(requestId, resolve));
		emit({
			type: 'control_request',
			request_id: requestId,
			request: {
				subtype: 'can_use_tool',
				tool_name: e.tool,
				input: e.input,
				tool_use_id: e.tool_use_id
			}
		});
		next.signal.addEventListener('abort', () => {
			pendingAsks.delete(requestId);
			emit({ type: 'control_cancel_request', request_id: requestId });
		});
		const { behavior, message } = await answer;
		return behavior === 'allow'
			? { decision: 'allow' }
			: { decision: 'deny', reason: message || 'Denied in Workbench chat' };
	});
};
