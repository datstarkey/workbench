import type {
	EngineInterface,
	PermissionRequestDecision,
	Register,
	SessionRateLimit,
	TurnStepInput
} from 'claude-code';

// Runs this interactive `claude` as a Workbench chat too: what `claude -p`
// would print as stream-json is posted to the server (`/mod/out`), and what it
// would read is long-polled (`/mod/in`), so the chat view and the terminal are
// one process. Workbench sets both variables on terminal shells it starts;
// outside Workbench the module does nothing.

type Line = Record<string, unknown>;
type Answer = {
	behavior?: string;
	message?: string;
	updatedInput?: Record<string, unknown>;
	updatedPermissions?: Extract<
		PermissionRequestDecision,
		{ behavior: 'allow' }
	>['updatedPermissions'];
};

interface Link {
	url: string;
	token: string;
	sessionId: string;
}

let link: Link | null = null;
let outbox: Line[] = [];
let flushing: Promise<void> | null = null;
let polling = false;
let model = '';
// The model and effort last reported, so a change is sent once.
let lastSettings = '';
// The live permission mode, and the one the chat was last told.
let liveMode: string | undefined;
let reportedMode = '';
let messageSeq = 0;
let currentMessage = '';
const startedBlocks = new Set<number>();
let askSeq = 0;
// The server lost this session (it restarted, or a stop detached it): say hello again.
let needsHello = false;
let lastHello = 0;
// Effort picked in chat; applied to each main-thread model request until the
// TUI picks another, which then wins as the later pick does in the TUI.
let effort: TurnStepInput['effort'];
// The effort the engine resolved on its own when the chat's was applied first.
let effortBase: { value: TurnStepInput['effort'] } | undefined;
// The running main-thread turn, for an interrupt from chat.
let runningTurn: string | undefined;
// The live model's context window, from the latest measurement.
let contextWindow: number | undefined;
// Why the last turn failed (StopFailure), and the `result` that reported it,
// re-sent with the reason when StopFailure comes after the turn's end.
let failure: string | undefined;
let failedResult: Line | undefined;
// The parts of tool results the transcript row doesn't carry (an edit's patch,
// an Artifact's link), by call id, until the row and the result have met.
const toolResults = new Map<string, Line>();
const toolRows = new Map<string, Line>();
const callsRunning = new Set<string>();
const TOOL_RESULTS_KEPT = 50;
// Main-thread Agent calls still running (task ids = tool call ids); progress
// goes to the only one when a subagent wasn't linked.
const runningAgents = new Set<string>();
// Running subagents: agent id <-> task id, for progress and their output file.
const agentTasks = new Map<string, string>();
const taskAgents = new Map<string, string>();
// Background agents: their Agent call returned at launch, so their own
// `turn.complete` ends the task.
const asyncAgents = new Set<string>();
// Chat prompts appended into the running turn that no request has read yet.
let injected: string[] = [];
// Prompts the plugin submitted itself, kept out of the chat (each echoes once).
const echoed = new Set<string>();

// Questions and plans are asked from `tool.call` (an answer edits the input);
// `tool.check` then lets the answered call through.
const ASKED_IN_CALL = new Set(['AskUserQuestion', 'ExitPlanMode']);
// Calls core put to the mode's decider, by tool and input: the decider's
// dialog (`PermissionRequest`) names no call. A rule, the mode or auto mode's
// classifier settles most without one, so only the newest are kept.
const pendingAsks = new Map<string, { id: string; reason?: string }>();
const PENDING_ASKS_KEPT = 50;
const answeredInChat = new Set<string>();
// Approvals the terminal's own dialog asks (no chat was open): tool call id → request id.
const askedInTerminal = new Map<string, string>();
// MCP elicitations the terminal shows, by server and elicitation id, oldest first.
const terminalElicitations = new Map<string, string[]>();
// Background jobs reported to the tasks panel, by id, with their last status.
const backgroundJobs = new Map<string, string>();
// `/clear` or `/resume` ended the session and the process carries on under another id.
let rekeyPending = false;

/**
 * Sent when chat prompts put into a turn were never read by it: the model
 * finds them in the conversation already. Core's `QUEUED_NUDGE` hides it.
 */
const QUEUED_NUDGE = 'Please answer the message I sent while you were working.';
const ATTACHED_FILES = '\n\nAttached files (read each with the Read tool):\n';

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

// Queued, then posted one request at a time, so lines reach the server in
// the order they happened.
function emit(...lines: Line[]) {
	if (link) outbox.push(...lines);
}

function takeOutbox() {
	const lines = outbox;
	outbox = [];
	return { sessionId: link?.sessionId, lines };
}

/** Post what is queued, after any post in flight. */
async function flush($: EngineInterface) {
	while (flushing) await flushing;
	if (outbox.length === 0 || !link) return;
	flushing = $.http
		.fetch(`${link.url}/mod/out`, init('POST', takeOutbox()))
		.then((res) => {
			if (res.status === 404) needsHello = true;
		})
		.catch(() => {})
		.finally(() => (flushing = null));
	await flushing;
}

/** After `/clear` or `/resume` the server hears of the new id before any of its lines. */
async function rekey($: EngineInterface) {
	if (!rekeyPending || !link) return;
	const id = await $.session.id();
	if (!rekeyPending || !link || id === link.sessionId) return;
	rekeyPending = false;
	// The server finds the session by token until the reset re-keys it.
	emit({ type: 'conversation_reset', new_conversation_id: id, session_id: link.sessionId });
	link = { ...link, sessionId: id };
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

// What the transcript reads of a tool's structured result: an edit's patch,
// an Artifact's link. The rest (whole files, outputs) stays out of the wire.
const RESULT_KEYS = [
	'structuredPatch',
	'url',
	'title',
	'version',
	'created_from_type',
	'updated',
	'opened'
];

function slimResult(result: unknown): Line | undefined {
	if (!result || typeof result !== 'object') return undefined;
	const kept = Object.entries(result).filter(([k]) => RESULT_KEYS.includes(k));
	return kept.length ? Object.fromEntries(kept) : undefined;
}

const askKey = (tool: string, input: unknown) => `${tool}\0${JSON.stringify(input)}`;

/** The pending call a `PermissionRequest` is about. */
function takeAsk(tool: string, input: unknown) {
	const key = askKey(tool, input);
	const ask = pendingAsks.get(key);
	pendingAsks.delete(key);
	return ask;
}

function askLine(
	requestId: string,
	tool: string,
	input: unknown,
	toolUseId?: string,
	reason?: string,
	suggestions?: unknown
): Line {
	return {
		type: 'control_request',
		request_id: requestId,
		request: {
			subtype: 'can_use_tool',
			tool_name: tool,
			input,
			tool_use_id: toolUseId,
			description: reason,
			permission_suggestions: suggestions
		}
	};
}

/**
 * A prompt naming files to Read: a plugin's prompt never has its `@` mentions
 * (chat images and files) expanded, submitted or appended.
 */
function withAttachments(text: string): string {
	const files = [...text.matchAll(/(?:^|\s)@(?:"([^"]+)"|(\S+))/g)].map((m) => m[1] ?? m[2]);
	return files.length ? `${text}${ATTACHED_FILES}${files.map((f) => `- ${f}`).join('\n')}` : text;
}

/**
 * How the CLI frames a prompt typed while a turn runs, so the model takes it
 * up. Core's `QUEUED_PROMPT_PREFIX` unwraps this wording when history is reloaded.
 */
function midTurn(text: string): string {
	return `The user sent a new message while you were working:\n${withAttachments(text)}\n\nIMPORTANT: After completing your current task, you MUST address the user's message above. Do not ignore it.`;
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
async function askInChat(
	$: EngineInterface,
	requestId: string,
	line: Line,
	signal: AbortSignal
): Promise<Answer | null> {
	// The tool's card reaches the chat before its approval does.
	await flush($);
	let first: Line | undefined = line;
	let answer: Answer | null | undefined;
	while (answer === undefined && link && !signal.aborted) {
		const res = await untilAborted(
			signal,
			$.http
				.fetch(
					`${link.url}/mod/ask`,
					init('POST', { sessionId: link.sessionId, requestId, line: first })
				)
				.catch(() => null)
		);
		first = undefined;
		if (!signal.aborted) answer = askAnswer(res?.ok ? res.text : undefined);
	}
	if (signal.aborted) emit({ type: 'control_cancel_request', request_id: requestId });
	return answer ?? null;
}

/**
 * The session's live permission mode, from a classic hook's input: the
 * `/config` row holds only the settings default (`defaultMode`), not a mode
 * the session was launched in or switched to.
 */
export function notePermissionMode(mode: string | undefined) {
	if (!mode) return;
	liveMode = mode;
	if (!link || mode === reportedMode) return;
	reportedMode = mode;
	emit({ type: 'permission-mode', permissionMode: mode });
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

/** The most used rate-limit window, as the SDK's `rate_limit_event` reports one. */
function rateLimitLine(windows: readonly SessionRateLimit[]): Line | undefined {
	const top = [...windows].sort((a, b) => b.percentUsed - a.percentUsed)[0];
	if (!top) return undefined;
	// The SDK warns past its own thresholds; 90% is where the chat starts to.
	const status =
		top.percentUsed >= 100 ? 'rejected' : top.percentUsed >= 90 ? 'allowed_warning' : 'allowed';
	const resets = top.resetsAt ? Date.parse(top.resetsAt) : NaN;
	return {
		type: 'rate_limit_event',
		rate_limit_info: {
			status,
			rateLimitType: top.kind,
			utilization: top.percentUsed / 100,
			...(Number.isNaN(resets) ? {} : { resetsAt: Math.floor(resets / 1000) })
		}
	};
}

/** One long-poll for what the chat sends, run by the poll timer. */
function poll($: EngineInterface) {
	if (polling || !link) return;
	polling = true;
	void rekey($);
	if (needsHello && Date.now() - lastHello > 5000) {
		needsHello = false;
		lastHello = Date.now();
		lastSettings = '';
		reportedMode = '';
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
				if (line.type === 'user') await chatPrompt($, line);
				else if (line.type === 'control_request') await controlRequest($, line);
			}
		})
		.catch(() => {})
		.finally(() => (polling = false));
}

async function chatPrompt($: EngineInterface, line: Line) {
	const message = line.message as { content?: unknown } | undefined;
	const text = promptText(message?.content);
	if (!text) return;
	// A plugin's submit waits for the turn to end, so mid-turn it joins the
	// running turn the way a typed prompt would.
	const appended = runningTurn
		? await $.session
				.append({ message: { type: 'user', content: [{ type: 'text', text: midTurn(text) }] } })
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
		// Not awaited: it resolves when its turn starts, and polling must go on
		// meanwhile (approval answers, interrupts).
		void $.prompt.submit({ text: withAttachments(text), asUser: true });
	}
}

async function controlRequest($: EngineInterface, line: Line) {
	const req = (line.request ?? {}) as {
		subtype?: string;
		model?: string;
		settings?: { effortLevel?: string };
	};
	const sub = req.subtype;
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
	} else if (sub === 'apply_flag_settings' && req.settings?.effortLevel) {
		effort = req.settings.effortLevel as TurnStepInput['effort'];
		effortBase = undefined;
		reply(line.request_id);
	} else {
		reply(line.request_id, `${sub ?? 'This'} isn't available in a terminal session yet.`);
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
		$.clock.every(50, () => {
			if (!flushing) void flush($);
		});
		$.clock.every(300, () => poll($));
		const rows = await $.config.list().catch(() => []);
		const configMode = rows.find((r) => r.key === 'permissionMode')?.value;
		const permissionMode = liveMode ?? (typeof configMode === 'string' ? configMode : undefined);
		reportedMode = permissionMode ?? '';
		emit({ type: 'system', subtype: 'init', session_id: sessionId, model, permissionMode });
		return result;
	});

	on('session.end', async ($, e, next) => {
		if (link && (e.reason === 'clear' || e.reason === 'resume')) {
			rekeyPending = true;
			for (const task of [...taskAgents.keys()]) unlinkTask(task);
			return next(e);
		}
		if (link) {
			const { url } = link;
			// One request, inside the short bound an exit gets.
			const bye = init('POST', takeOutbox());
			link = null;
			const left = Math.max(0, next.budget.remainingMs - 100);
			await Promise.race([
				(async () => {
					while (flushing) await flushing;
					await $.http.fetch(`${url}/mod/bye`, bye).catch(() => {});
				})(),
				$.clock.sleep(left)
			]);
		}
		return next(e);
	});

	on('session.measure', ($, e, next) => {
		if (link) {
			contextWindow = e.context.window;
			const limit = e.changed.includes('rateLimits') ? rateLimitLine(e.rateLimits) : undefined;
			if (limit) emit(limit);
		}
		return next(e);
	});

	// A subagent's run raises none: this is always the main thread's turn.
	on('turn.start', ($, e, next) => {
		runningTurn = e.turnId;
		failure = undefined;
		failedResult = undefined;
		return next(e);
	});

	on('turn.step', async function* ($, e, next) {
		if (!link || e.agentId) return yield* next(e);
		await rekey($);
		// This request carries every row appended so far.
		injected = [];
		if (effort) {
			if (!effortBase) effortBase = { value: e.effort };
			else if (e.effort !== effortBase.value) effort = effortBase = undefined;
		}
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
				emit({
					type: 'stream_event',
					event: {
						type: 'content_block_start',
						index: chunk.index,
						content_block: { type: 'tool_use', id: chunk.id, name: chunk.name, input: {} }
					}
				});
			} else if (chunk.kind === 'stop') {
				if (chunk.usage?.model) model = chunk.usage.model;
				// The response's rows were appended block by block before its usage
				// was known: the context size and cache timer read it from here.
				if (chunk.usage)
					emit({
						type: 'assistant',
						uuid: `${currentMessage}-usage`,
						session_id: link?.sessionId,
						message: { id: currentMessage, model, content: [], usage: chunk.usage }
					});
			}
			yield chunk;
		}
		return stream.result;
	});

	on('session.append', async ($, e, next) => {
		const m = e.message;
		if (!link || e.agentId) return next(e);
		await rekey($);
		if (!link) return next(e);
		if (m.type === 'attachment' && m.name === 'queued_command') {
			// A prompt typed in the terminal while a turn ran, folded into that turn.
			const text = promptText(m.content);
			if (text)
				emit({
					type: 'user',
					uuid: e.uuid,
					session_id: link.sessionId,
					message: { role: 'user', content: text }
				});
		} else if (m.isMeta) {
			return next(e);
		} else if (m.type === 'assistant' && e.door === 'response') {
			emit({
				type: 'assistant',
				uuid: e.uuid,
				session_id: link.sessionId,
				message: { ...m, id: currentMessage || `wbmod-${e.uuid}`, model }
			});
		} else if (m.type === 'user' && e.door === 'prompt') {
			if (echoed.delete(promptText(m.content))) return next(e);
			emit({ type: 'user', uuid: e.uuid, session_id: link.sessionId, message: m });
		} else if (m.type === 'user' && e.door === 'tool-result') {
			const row: Line = { type: 'user', uuid: e.uuid, session_id: link.sessionId, message: m };
			const id = (m.content as { tool_use_id?: string }[] | undefined)?.find(
				(b) => b.tool_use_id
			)?.tool_use_id;
			const result = id ? toolResults.get(id) : undefined;
			if (id) toolResults.delete(id);
			// Still running: its structured result is sent with the row once it returns.
			if (id && result === undefined && callsRunning.has(id)) toolRows.set(id, row);
			emit(result === undefined ? row : { ...row, tool_use_result: result });
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
			if (e.reason === 'refusal')
				emit({
					type: 'system',
					subtype: 'model_refusal_no_fallback',
					original_model: model,
					api_refusal_explanation: e.refusal.explanation,
					uuid: `wbmod-refusal-${++askSeq}`
				});
			const modelUsage = contextWindow ? { [model]: { contextWindow } } : undefined;
			if (e.reason === 'error') {
				failedResult = {
					type: 'result',
					subtype: 'error_during_execution',
					is_error: true,
					result: failure ?? 'The turn failed.',
					uuid: `wbmod-result-${++askSeq}`,
					modelUsage
				};
				emit(failedResult);
			} else {
				emit({ type: 'result', subtype: 'success', is_error: false, modelUsage });
			}
			const unread = injected.length > 0;
			injected = [];
			// Entries live one turn at most, so a stale one can't hide a later prompt.
			echoed.clear();
			// Stopped: the prompt stays in the chat, as an interrupted CLI leaves it.
			if (e.reason === 'aborted' || !unread) return next(e);
			// The unread prompts are in the conversation already; only ask for an answer.
			echoed.add(QUEUED_NUDGE);
			void $.prompt.submit({ text: QUEUED_NUDGE, asUser: true });
		}
		return next(e);
	});

	// An API error ended the turn: the chat says why instead of a bare failure.
	on('classic.StopFailure', ($, e, next) => {
		if (link) {
			failure = e.error_details || `The turn failed: ${e.error.replace(/_/g, ' ')}.`;
			if (failedResult) emit({ ...failedResult, result: failure });
		}
		return next(e);
	});

	// `/model`, the `/config` row (the chat's own `set_model` too) or a fallback.
	on('classic.PostModelSwitch', ($, e, next) => {
		if (link) {
			model = e.to_model;
			emit({
				type: 'system',
				subtype: 'init',
				model: e.to_model,
				modelChoice: e.requested_model ?? 'default'
			});
		}
		return next(e);
	});

	on('classic.PermissionDenied', ($, e, next) => {
		notePermissionMode(e.permission_mode);
		if (link && !e.agent_id)
			emit({
				type: 'system',
				subtype: 'permission_denied',
				tool_name: e.tool_name,
				tool_use_id: e.tool_use_id,
				decision_reason: e.reason,
				uuid: `wbmod-denied-${e.tool_use_id}`
			});
		return next(e);
	});

	// An MCP server asks for input. The terminal's dialog answers it; the chat
	// shows a read-only card, and the session waits on it until it's answered.
	on('classic.Elicitation', ($, e, next) => {
		if (link) {
			const id = `wbmod-elicit-${++askSeq}`;
			const key = `${e.mcp_server_name}\0${e.elicitation_id ?? ''}`;
			terminalElicitations.set(key, [...(terminalElicitations.get(key) ?? []), id]);
			emit({
				type: 'workbench_terminal_elicitation',
				id,
				mcp_server_name: e.mcp_server_name,
				message: e.message,
				mode: e.mode,
				url: e.url,
				requested_schema: e.requested_schema
			});
		}
		return next(e);
	});

	on('classic.ElicitationResult', ($, e, next) => {
		const key = `${e.mcp_server_name}\0${e.elicitation_id ?? ''}`;
		const id = terminalElicitations.get(key)?.shift();
		if (terminalElicitations.get(key)?.length === 0) terminalElicitations.delete(key);
		if (link && id) emit({ type: 'workbench_terminal_elicitation_result', id, action: e.action });
		return next(e);
	});

	// A command that runs no model turn (`/cost`, a forked skill like `/code-review`)
	// prints instead: a chat that sent it waits for that and a `result`, as `-p`
	// prints them. A forked skill's agent has no `agent.spawn`, so it joins the
	// tasks panel here; its own `turn.complete` ends the task.
	on('command.run', async ($, e, next) => {
		if (!link) return next(e);
		await rekey($);
		const before = new Set((await $.agent.list()).map((a) => a.id));
		const result = await next(e);
		if (!link) return result;
		await rekey($);
		for (const agent of await $.agent.list()) {
			if (before.has(agent.id) || agent.parentId || agentTasks.has(agent.id)) continue;
			linkAgent(agent.id, agent.id);
			asyncAgents.add(agent.id);
			emit(
				{
					type: 'system',
					subtype: 'task_started',
					task_id: agent.id,
					description: agent.description || `/${e.command}`,
					subagent_type: agent.type,
					task_type: 'local_agent',
					uuid: `wbmod-task-${agent.id}`
				},
				{
					type: 'system',
					subtype: 'task_updated',
					task_id: agent.id,
					output_id: agent.id,
					uuid: `wbmod-task-out-${agent.id}`
				}
			);
		}
		if (result.text !== undefined) {
			emit({
				type: 'system',
				subtype: 'local_command_output',
				content: result.text,
				uuid: `wbmod-command-${++askSeq}`
			});
			const fromChat = e.origin.kind === 'plugin' && e.origin.name === 'workbench';
			if (fromChat && !runningTurn) emit({ type: 'result', subtype: 'success', is_error: false });
		}
		return result;
	});

	// `/compact` (the chat's, or the server's upkeep) runs no turn either: the chat
	// needs the boundary and a `result`. Auto compaction runs inside a turn, which
	// ends with its own.
	on('session.compact', async ($, e, next) => {
		const result = await next(e);
		if (!link || e.agentId || e.trigger === 'precompute') return result;
		if (!result.messages) {
			emit({
				type: 'system',
				subtype: 'local_command_output',
				content: result.skip,
				uuid: `wbmod-compact-${++askSeq}`
			});
		} else {
			emit({
				type: 'system',
				subtype: 'compact_boundary',
				compact_metadata: { trigger: e.trigger, pre_tokens: result.tokensBefore },
				uuid: `wbmod-compact-${++askSeq}`
			});
		}
		if (!runningTurn) emit({ type: 'result', subtype: 'success', is_error: false });
		return result;
	});

	// Structured results (an Artifact's link) and subagents for the tasks panel.
	on('tool.call', async ($, e, next) => {
		if (!link) return next(e);
		const id = e.tool_use_id;
		// Approved in the terminal's dialog: it no longer waits, however long the call runs.
		const asked = id ? askedInTerminal.get(id) : undefined;
		if (id && asked) {
			askedInTerminal.delete(id);
			emit({ type: 'control_cancel_request', request_id: asked });
		}
		if (ASKED_IN_CALL.has(e.tool) && !e.agentId && id) {
			const { tool: _tool, tool_use_id: _id, agentId: _agent, ...input } = e;
			const requestId = `wbmod-ask-${++askSeq}`;
			const answer = await askInChat(
				$,
				requestId,
				askLine(requestId, e.tool, input, id),
				next.signal
			);
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
		const main = id && !e.agentId ? id : undefined;
		if (main) callsRunning.add(main);
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
		if (main) {
			callsRunning.delete(main);
			const slim = slimResult(result.result);
			const row = toolRows.get(main);
			toolRows.delete(main);
			if (row && slim) emit({ ...row, tool_use_result: slim });
			else if (slim) {
				// The row comes after the call returns.
				toolResults.set(main, slim);
				if (toolResults.size > TOOL_RESULTS_KEPT)
					toolResults.delete(toolResults.keys().next().value!);
			}
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
		if (verdict.decision === 'ask' && link && e.tool_use_id && !ASKED_IN_CALL.has(e.tool)) {
			const key = askKey(e.tool, e.input);
			pendingAsks.delete(key);
			pendingAsks.set(key, { id: e.tool_use_id, reason: verdict.reason });
			if (pendingAsks.size > PENDING_ASKS_KEPT)
				pendingAsks.delete(pendingAsks.keys().next().value!);
		}
		return verdict;
	});

	// Fires only when the mode's decider would show its dialog: rules, the mode
	// and auto mode's classifier have all had their say. Asked in chat while one
	// is open; the server answers `fallback` when none is (or it closes), and the
	// terminal's dialog asks instead. A held request in flight doesn't spend the
	// hook's time budget, however long the person takes.
	on('classic.PermissionRequest', async ($, e, next) => {
		notePermissionMode(e.permission_mode);
		if (!link || ASKED_IN_CALL.has(e.tool_name)) return next(e);
		const pending = takeAsk(e.tool_name, e.tool_input);
		const requestId = `wbmod-ask-${++askSeq}`;
		const line = askLine(
			requestId,
			e.tool_name,
			e.tool_input,
			pending?.id,
			pending?.reason,
			e.permission_suggestions
		);
		const answer = await askInChat($, requestId, line, next.signal);
		if (!answer) {
			// The terminal asks now; the server shows it waiting until it's answered.
			if (pending && !next.signal.aborted) askedInTerminal.set(pending.id, requestId);
			return next(e);
		}
		const decision: PermissionRequestDecision =
			answer.behavior === 'allow'
				? { behavior: 'allow', updatedPermissions: answer.updatedPermissions }
				: { behavior: 'deny', message: answer.message || 'Denied in Workbench chat' };
		return { decision };
	});
};
