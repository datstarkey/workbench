import type { Register, TurnStepInput } from 'claude-code';

// Runs this interactive `claude` as a Workbench chat too: what `claude -p`
// would print as stream-json is posted to the server (`/mod/out`), and what it
// would read is long-polled (`/mod/in`), so the chat view and the terminal are
// one process. Workbench sets both variables on terminal shells it starts;
// outside Workbench the module does nothing.

type Line = Record<string, unknown>;
type Answer = { behavior?: string; message?: string; updatedInput?: Record<string, unknown> };

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
// The model and effort last reported, so a change is sent once.
let lastSettings = '';
let messageSeq = 0;
let currentMessage = '';
const startedBlocks = new Set<number>();
let askSeq = 0;
// The server lost this session (it restarted, or a stop detached it): say hello again.
let needsHello = false;
let lastHello = 0;
// Effort picked in chat; applied to each main-thread model request.
let effort: TurnStepInput['effort'];
// The running main-thread turn, for an interrupt from chat.
let runningTurn: string | undefined;
// The last request's token counts, stamped on its assistant rows (context
// size and the prompt-cache timer read them).
let lastUsage: unknown;
// Tool results the transcript row doesn't carry (an Artifact's link), by call id.
const toolResults = new Map<string, unknown>();
const toolRows = new Map<string, Line>();
// Main-thread Agent calls still running (task ids = tool call ids); progress
// goes to the only one when a subagent wasn't linked.
const runningAgents = new Set<string>();
// Running subagents: agent id <-> task id, for progress and their output file.
const agentTasks = new Map<string, string>();
const taskAgents = new Map<string, string>();
// Background agents: their Agent call returned at launch, so their own
// `turn.complete` ends the task.
const asyncAgents = new Set<string>();
// Chat prompts appended into the running turn that no request has read yet;
// any left when the turn ends run as their own turn (echoed once already).
let injected: string[] = [];
const echoed = new Set<string>();

// Questions and plans are asked from `tool.call` (an answer edits the input);
// `tool.check` then lets the answered call through.
const ASKED_IN_CALL = new Set(['AskUserQuestion', 'ExitPlanMode']);
const answeredInChat = new Set<string>();
// Background jobs reported to the tasks panel, by id, with their last status.
const backgroundJobs = new Map<string, string>();
// `/clear` ends the session and the process carries on under a new id.
let clearPending = false;

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

function reply(requestId: unknown, error?: string, response: Line = {}) {
	emit({
		type: 'control_response',
		response: error
			? { subtype: 'error', request_id: requestId, error }
			: { subtype: 'success', request_id: requestId, response }
	});
}

const EFFORT_LEVELS = ['low', 'medium', 'high', 'xhigh', 'max'];

/** A `/config` model value as the chat's picker shows it. */
function modelOption(value: string) {
	const wide = value.endsWith('[1m]');
	const base = value.replace('[1m]', '');
	const name = base === 'opusplan' ? 'Opus Plan' : base.charAt(0).toUpperCase() + base.slice(1);
	return {
		value,
		displayName: wide ? `${name} (1M context)` : name,
		...(base === 'haiku' ? {} : { supportedEffortLevels: EFFORT_LEVELS })
	};
}

function linkAgent(agent: string, task: string) {
	agentTasks.set(agent, task);
	taskAgents.set(task, agent);
}

function unlinkTask(task: string) {
	const agent = taskAgents.get(task);
	taskAgents.delete(task);
	if (agent) {
		agentTasks.delete(agent);
		asyncAgents.delete(agent);
	}
}

function denial(result: { deny?: string } | undefined, what: string): string | undefined {
	return result?.deny ? `${what}: ${result.deny}` : undefined;
}

// The server turns attachments into `@path` mentions, so a prompt is text.
function promptText(content: unknown): string {
	if (typeof content === 'string') return content;
	const blocks = Array.isArray(content) ? (content as { type?: string; text?: string }[]) : [];
	return blocks
		.filter((b) => b.type === 'text')
		.map((b) => b.text ?? '')
		.join('\n');
}

function askLine(requestId: string, tool: string, input: unknown, toolUseId?: string): Line {
	return {
		type: 'control_request',
		request_id: requestId,
		request: { subtype: 'can_use_tool', tool_name: tool, input, tool_use_id: toolUseId }
	};
}

/**
 * How the CLI frames a prompt typed while a turn runs, so the model takes it up.
 * `@` mentions (chat images and files) are only expanded for a submitted
 * prompt, so an appended one names them for the model to Read. Core's
 * `QUEUED_PROMPT_PREFIX` unwraps this wording when history is reloaded.
 */
function midTurn(text: string): string {
	const files = [...text.matchAll(/(?:^|\s)@(?:"([^"]+)"|(\S+))/g)].map((m) => m[1] ?? m[2]);
	const attached = files.length
		? `\n\nAttached files (read each with the Read tool):\n${files.map((f) => `- ${f}`).join('\n')}`
		: '';
	return `The user sent a new message while you were working:\n${text}${attached}\n\nIMPORTANT: After completing your current task, you MUST address the user's message above. Do not ignore it.`;
}

/**
 * One `/mod/ask` reply: the answer, `null` to fall back to the terminal (no
 * chat open, or the server is unreachable), or `undefined` to keep waiting.
 */
function askAnswer(text: string | undefined): Answer | null | undefined {
	if (text === undefined) return null;
	const reply = JSON.parse(text) as {
		answer?: { response?: { subtype?: string; response?: Answer; error?: string } };
		fallback?: boolean;
	};
	if (reply.fallback) return null;
	const response = reply.answer?.response;
	if (!response) return undefined;
	return response.subtype === 'error'
		? { behavior: 'deny', message: response.error }
		: (response.response ?? { behavior: 'deny' });
}

/** Report background jobs that finished; called from the Stop hook with its job list. */
export function noteBackgroundTasks(tasks: readonly { id: string; status: string }[] = []) {
	if (!link) return;
	const live = new Map(tasks.map((t) => [t.id, t.status]));
	for (const [job, status] of backgroundJobs) {
		const now = live.get(job) ?? 'completed';
		if (now === status) continue;
		backgroundJobs.set(job, now);
		if (now !== 'running')
			emit({
				type: 'system',
				subtype: 'task_notification',
				task_id: job,
				status: now === 'failed' ? 'failed' : 'completed',
				uuid: `wbmod-bg-done-${job}`
			});
	}
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
		const rows = await $.config.list();
		const permissionMode = rows.find((r) => r.key === 'permissionMode')?.value;
		emit({ type: 'system', subtype: 'init', session_id: sessionId, model, permissionMode });

		$.clock.every(50, () => {
			if (flushing || outbox.length === 0 || !link) return;
			flushing = true;
			$.http
				.fetch(`${link.url}/mod/out`, init('POST', takeOutbox()))
				.then((res) => {
					if (res.status === 404) needsHello = true;
				})
				.catch(() => {})
				.finally(() => (flushing = false));
		});

		$.clock.every(300, () => {
			if (polling || !link) return;
			polling = true;
			if (clearPending) {
				// The server finds the session by token until the reset re-keys it.
				void $.session.id().then((id) => {
					if (!link || id === link.sessionId) return;
					clearPending = false;
					emit({ type: 'conversation_reset', new_conversation_id: id, session_id: link.sessionId });
					link = { ...link, sessionId: id };
				});
			}
			if (needsHello && Date.now() - lastHello > 5000) {
				needsHello = false;
				lastHello = Date.now();
				lastSettings = '';
				void $.http
					.fetch(`${link.url}/mod/hello`, init('POST', { sessionId: link.sessionId }))
					.then((res) => {
						if (!res.ok) needsHello = true;
					})
					.catch(() => (needsHello = true));
			}
			$.http
				.fetch(`${link.url}/mod/in?${sessionQuery()}`, init('GET'))
				.then(async (res) => {
					if (res.status === 404) needsHello = true;
					if (!res.ok) return;
					for (const line of JSON.parse(res.text || '[]') as Line[]) {
						if (line.type === 'user') {
							const message = line.message as { content?: unknown } | undefined;
							const text = promptText(message?.content);
							if (!text) continue;
							// A plugin's submit waits for the turn to end, so mid-turn it
							// joins the running turn the way a typed prompt would.
							const appended = runningTurn
								? await $.session
										.append({
											message: { type: 'user', content: [{ type: 'text', text: midTurn(text) }] }
										})
										.catch((err: unknown) => ({ deny: String(err) }))
								: undefined;
							if (appended && !appended.deny && runningTurn) {
								injected.push(text);
								emit({
									type: 'user',
									uuid: `wbmod-queued-${++askSeq}`,
									session_id: link?.sessionId,
									message: { role: 'user', content: text }
								});
							} else {
								// Not awaited: it resolves when its turn starts, and polling
								// must go on meanwhile (approval answers, interrupts).
								void $.prompt.submit({ text, asUser: true });
							}
						} else if (line.type === 'control_request') {
							const sub = (line.request as { subtype?: string } | undefined)?.subtype;
							const req = (line.request ?? {}) as {
								model?: string;
								mode?: string;
								settings?: { effortLevel?: string };
							};
							if (sub === 'interrupt') {
								if (runningTurn) await $.turn.abort({ turnId: runningTurn }).catch(() => {});
								reply(line.request_id);
							} else if (sub === 'initialize') {
								// A fallback list: the server replaces it with the CLI's own.
								const row = (await $.config.list()).find((r) => r.key === 'model');
								const models = (row?.options ?? []).map(modelOption);
								const commands = (await $.command.list()).map((c) => ({
									name: c.name,
									description: c.description
								}));
								const modelChoice = typeof row?.value === 'string' ? row.value : undefined;
								reply(line.request_id, undefined, { models, commands, modelChoice });
							} else if (sub === 'set_model' && req.model) {
								// As the TUI's `/config` does (aliases resolve there; a plugin can't
								// switch a session to an alias on its own).
								const set = await $.config.set({ key: 'model', value: req.model });
								reply(line.request_id, denial(set, 'Model'));
							} else if (sub === 'set_permission_mode' && req.mode) {
								const set = await $.config
									.set({ key: 'permissionMode', value: req.mode })
									.catch((err: unknown) => ({ deny: String(err) }));
								reply(line.request_id, denial(set, 'Mode'));
							} else if (sub === 'apply_flag_settings' && req.settings?.effortLevel) {
								effort = req.settings.effortLevel as TurnStepInput['effort'];
								reply(line.request_id);
							} else if (sub === 'rewind_files') {
								// A plugin has no way to restore Claude's file checkpoints; a
								// `canRewind: false` result shows as the rewind panel's own note.
								reply(line.request_id, undefined, {
									canRewind: false,
									error:
										"files can't be restored in a terminal chat yet. Rewind the conversation only, or undo the changes with git."
								});
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
		if (link && e.reason === 'clear') {
			clearPending = true;
			for (const task of [...taskAgents.keys()]) unlinkTask(task);
			return next(e);
		}
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
		if (!link || e.agentId) return yield* next(e);
		runningTurn = e.turnId;
		// This request carries every row appended so far.
		injected = [];
		const stream = next(effort ? { ...e, effort } : e);
		currentMessage = `wbmod-${link.sessionId.slice(0, 8)}-${++messageSeq}`;
		startedBlocks.clear();
		model = e.model || model;
		// What this request really runs with: the engine resolved both.
		const sent = effort ?? e.effort;
		const settings = `${model} ${sent ?? ''}`;
		if (settings !== lastSettings) {
			lastSettings = settings;
			emit({
				type: 'system',
				subtype: 'init',
				model,
				...(typeof sent === 'string' ? { effort: sent } : {})
			});
		}
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
			} else if (chunk.kind === 'stop') {
				lastUsage = chunk.usage ?? undefined;
				if (chunk.usage?.model) model = chunk.usage.model;
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
					message: {
						...m,
						id: currentMessage || `wbmod-${e.uuid}`,
						model,
						...(lastUsage ? { usage: lastUsage } : {})
					}
				});
			} else if (m.type === 'user' && e.door === 'prompt') {
				const text = promptText(m.content);
				if (echoed.delete(text)) return next(e);
				emit({ type: 'user', uuid: e.uuid, session_id: link.sessionId, message: m });
			} else if (m.type === 'user' && e.door === 'tool-result') {
				const row: Line = { type: 'user', uuid: e.uuid, session_id: link.sessionId, message: m };
				const id = (m.content as { tool_use_id?: string }[] | undefined)?.find(
					(b) => b.tool_use_id
				)?.tool_use_id;
				if (id) toolRows.set(id, row);
				const result = id ? toolResults.get(id) : undefined;
				emit(result === undefined ? row : { ...row, tool_use_result: result });
			}
		}
		return next(e);
	});

	on('prompt.suggest', async ($, e, next) => {
		const shown = await next(e);
		if (link) emit({ type: 'prompt_suggestion', suggestion: e.text });
		return shown;
	});

	on('turn.complete', ($, e, next) => {
		const task = e.agentId && asyncAgents.has(e.agentId) ? agentTasks.get(e.agentId) : undefined;
		if (link && task && e.agentId) {
			unlinkTask(task);
			emit({
				type: 'system',
				subtype: 'task_notification',
				task_id: task,
				status: e.reason === 'aborted' ? 'stopped' : e.reason === 'answer' ? 'completed' : 'failed',
				uuid: `wbmod-done-${task}`
			});
		}
		if (link && !e.agentId) {
			runningTurn = undefined;
			emit({ type: 'result', subtype: 'success', is_error: e.reason === 'error' });
			const unread = injected;
			injected = [];
			// Entries live one turn at most, so a stale one can't hide a later prompt.
			echoed.clear();
			// Stopped: the prompt stays in the chat, as an interrupted CLI leaves it.
			if (e.reason === 'aborted') return next(e);
			for (const text of unread) {
				echoed.add(text);
				void $.prompt.submit({ text, asUser: true });
			}
		}
		return next(e);
	});

	// Structured results (an Artifact's link) and subagents for the tasks panel.
	on('tool.call', async ($, e, next) => {
		if (!link) return next(e);
		const id = e.tool_use_id;
		if (ASKED_IN_CALL.has(e.tool) && !e.agentId && id) {
			const { tool: _tool, tool_use_id: _id, agentId: _agent, ...input } = e;
			const requestId = `wbmod-ask-${++askSeq}`;
			let line: Line | undefined = askLine(requestId, e.tool, input, id);
			let answer: Answer | null | undefined;
			while (answer === undefined && link && !next.signal.aborted) {
				const res = await $.http
					.fetch(
						`${link.url}/mod/ask`,
						init('POST', { sessionId: link.sessionId, requestId, line })
					)
					.catch(() => null);
				line = undefined;
				answer = askAnswer(res?.ok ? res.text : undefined);
			}
			if (next.signal.aborted) emit({ type: 'control_cancel_request', request_id: requestId });
			if (answer) {
				if (answer.behavior !== 'allow')
					return { deny: answer.message || 'Declined in Workbench chat' };
				answeredInChat.add(id);
				return next({ ...e, ...(answer.updatedInput ?? {}) } as typeof e);
			}
		}
		const input = e as unknown as { description?: string; subagent_type?: string };
		const isAgent = !e.agentId && e.tool === 'Agent';
		if (isAgent && id) {
			runningAgents.add(id);
			emit({
				type: 'system',
				subtype: 'task_started',
				task_id: id,
				tool_use_id: id,
				description: input.description ?? 'Agent',
				subagent_type: input.subagent_type,
				task_type: 'local_agent',
				uuid: `wbmod-task-${id}`
			});
		} else if (e.agentId && (agentTasks.has(e.agentId) || runningAgents.size === 1)) {
			emit({
				type: 'system',
				subtype: 'task_progress',
				task_id: agentTasks.get(e.agentId) ?? [...runningAgents][0],
				last_tool_name: e.tool,
				uuid: `wbmod-progress-${++askSeq}`
			});
		}
		const result = await next(e);
		const launched = result.result as { status?: string; agentId?: string } | undefined;
		if (isAgent && id) {
			runningAgents.delete(id);
		}
		if (isAgent && id && launched?.status === 'async_launched' && launched.agentId) {
			asyncAgents.add(launched.agentId);
			linkAgent(launched.agentId, id);
		} else if (isAgent && id) {
			unlinkTask(id);
			emit({
				type: 'system',
				subtype: 'task_notification',
				task_id: id,
				status: result.deny || result.isError ? 'failed' : 'completed',
				uuid: `wbmod-done-${id}`
			});
		}
		const bg = (result.result as { backgroundTaskId?: string } | undefined)?.backgroundTaskId;
		if (e.tool === 'Bash' && bg && !backgroundJobs.has(bg)) {
			backgroundJobs.set(bg, 'running');
			emit({
				type: 'system',
				subtype: 'task_started',
				task_id: bg,
				tool_use_id: id,
				description: (e as { command?: string }).command ?? 'Background command',
				task_type: 'local_bash',
				is_backgrounded: true,
				uuid: `wbmod-bg-${bg}`
			});
		}
		if (id && !e.agentId && result.result !== undefined) {
			toolResults.set(id, result.result);
			const row = toolRows.get(id);
			if (row) emit({ ...row, tool_use_result: result.result });
		}
		return result;
	});

	on('agent.spawn', async ($, e, next) => {
		const result = await next(e);
		// The spawn names its Agent call; only the main thread's calls are tasks.
		const task = runningAgents.has(e.tool_use_id) ? e.tool_use_id : undefined;
		if (link && task && result.agentId) {
			linkAgent(result.agentId, task);
			// The CLI writes a subagent's log as `<agent id>.output`.
			emit({
				type: 'system',
				subtype: 'task_updated',
				task_id: task,
				output_id: result.agentId,
				uuid: `wbmod-task-out-${task}`
			});
		}
		return result;
	});

	on('tool.check', async ($, e, next) => {
		const verdict = await next(e);
		if (e.tool_use_id && answeredInChat.delete(e.tool_use_id)) return { decision: 'allow' };
		if (verdict.decision !== 'ask' || !link || ASKED_IN_CALL.has(e.tool)) return verdict;
		// Asked in chat while one is open; the server answers `fallback` when none is
		// (or it closes), and the terminal asks instead. A held request in flight
		// doesn't spend the hook's time budget, however long the person takes.
		const requestId = `wbmod-ask-${++askSeq}`;
		let line: Line | undefined = askLine(requestId, e.tool, e.input, e.tool_use_id);
		let answer: Answer | null | undefined;
		while (answer === undefined && link && !next.signal.aborted) {
			const res = await $.http
				.fetch(`${link.url}/mod/ask`, init('POST', { sessionId: link.sessionId, requestId, line }))
				.catch(() => null);
			line = undefined;
			answer = askAnswer(res?.ok ? res.text : undefined);
		}
		if (next.signal.aborted) emit({ type: 'control_cancel_request', request_id: requestId });
		if (!answer) return verdict;
		return answer.behavior === 'allow'
			? { decision: 'allow' }
			: { decision: 'deny', reason: answer.message || 'Denied in Workbench chat' };
	});
};
