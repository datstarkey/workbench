import type { PermissionRequestDecision, Register, TurnStepInput } from 'claude-code';
import * as server from './link';
import {
	askLine,
	midTurn,
	modelOption,
	promptText,
	QUEUED_NUDGE,
	QUEUED_PREFIX,
	rateLimitLine,
	slimResult,
	withAttachments,
	type Line
} from './lines';

// Runs this interactive `claude` as a Workbench chat too: what `claude -p`
// would print as stream-json is posted to the server (`/mod/out`), and what it
// would read is long-polled (`/mod/in`), so the chat view and the terminal are
// one process. Workbench sets both variables on terminal shells it starts;
// outside Workbench the module does nothing.

const { emit } = server;

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
// Effort picked in chat; applied to each main-thread model request until the
// TUI picks another, which then wins as the later pick does in the TUI.
let effort: TurnStepInput['effort'];
// The effort the engine resolved on its own when the chat's was applied first.
let effortBase: { value: TurnStepInput['effort'] } | undefined;
// The running main-thread turn, for an interrupt from chat.
let runningTurn: string | undefined;
// The live model's context window, from the latest measurement.
let contextWindow: number | undefined;
// Why the last turn failed (StopFailure), and the id of the `result` that
// reported it, whose notice takes the reason when StopFailure comes after.
let failure: string | undefined;
let failedResult: string | undefined;
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
// Main-thread Skill calls still running: call id → the skill and the agents
// there were before it. A forked skill's agent has no `agent.spawn`: the first
// call of an agent new since then names it, and its Skill call is its task.
const runningSkills = new Map<string, { skill: string; before: Set<string> }>();
// Whether the latest prompt came from a chat: what it asks waits for a chat's
// answer even before one has it open (a phone in the background).
let chatTurn = false;
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
// Model switches the person made (`/model`, `/config`, the chat's pick); an
// automatic fallback or a resume keeps the pick the chat shows.
const PICKED_MODEL = new Set(['command', 'picker', 'sdk']);

function reply(requestId: unknown, error?: string, response: Line = {}) {
	emit({
		type: 'control_response',
		response: error
			? { subtype: 'error', request_id: requestId, error }
			: { subtype: 'success', request_id: requestId, response }
	});
}

function linkAgent(agent: string, task: string) {
	agentTasks.set(agent, task);
	taskAgents.set(task, agent);
}

function startSkillTask(task: string, agent: string) {
	linkAgent(agent, task);
	emit(
		{
			type: 'system',
			subtype: 'task_started',
			task_id: task,
			tool_use_id: task,
			description: `/${runningSkills.get(task)?.skill ?? 'skill'}`,
			task_type: 'local_agent',
			uuid: `wbmod-task-${task}`
		},
		{
			type: 'system',
			subtype: 'task_updated',
			task_id: task,
			output_id: agent,
			uuid: `wbmod-task-out-${task}`
		}
	);
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

const askKey = (tool: string, input: unknown) => `${tool}\0${JSON.stringify(input)}`;

/** The pending call a `PermissionRequest` is about. */
function takeAsk(tool: string, input: unknown) {
	const key = askKey(tool, input);
	const ask = pendingAsks.get(key);
	pendingAsks.delete(key);
	return ask;
}

const elicitationKey = (e: { mcp_server_name: string; elicitation_id?: string }) =>
	`${e.mcp_server_name}\0${e.elicitation_id ?? ''}`;

/**
 * The session's live permission mode, from a classic hook's input: the
 * `/config` row holds only the settings default (`defaultMode`), not a mode
 * the session was launched in or switched to.
 */
export function notePermissionMode(mode: string | undefined) {
	if (!mode) return;
	liveMode = mode;
	if (!server.current() || mode === reportedMode) return;
	reportedMode = mode;
	emit({ type: 'permission-mode', permissionMode: mode });
}

/** Report background jobs that finished; called from the Stop hook with its job list. */
export function noteBackgroundTasks(tasks: readonly { id: string; status: string }[] = []) {
	if (!server.current()) return;
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
		server.open({ url, token, sessionId });
		const hello = await $.http
			.fetch(`${url}/mod/hello`, server.init('POST', { sessionId }))
			.catch(() => null);
		if (!hello?.ok) {
			server.open(null);
			return result;
		}

		// A chat prompt. A plugin's submit waits for the turn to end, so
		// mid-turn it joins the running turn the way a typed prompt would.
		const chatPrompt = async (line: Line) => {
			const message = line.message as { content?: unknown } | undefined;
			const text = promptText(message?.content);
			if (!text) return;
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
					session_id: server.current()?.sessionId,
					message: { role: 'user', content: text }
				});
			} else {
				// Not awaited: it resolves when its turn starts, and polling must go
				// on meanwhile (approval answers, interrupts).
				void $.prompt.submit({ text: withAttachments(text), asUser: true });
			}
		};

		const controlRequest = async (line: Line) => {
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
		};

		$.clock.every(50, () => {
			if (!server.isFlushing()) void server.flush((u, i) => $.http.fetch(u, i));
		});
		$.clock.every(300, () => {
			const link = server.current();
			if (polling || !link) return;
			polling = true;
			void server.rekey(() => $.session.id());
			if (server.hello.needed && Date.now() - server.hello.last > 5000) {
				server.hello.needed = false;
				server.hello.last = Date.now();
				lastSettings = '';
				reportedMode = '';
				void $.http
					.fetch(`${link.url}/mod/hello`, server.init('POST', { sessionId: link.sessionId }))
					.then((res) => {
						if (!res.ok) server.hello.needed = true;
					})
					.catch(() => (server.hello.needed = true));
			}
			$.http
				.fetch(`${link.url}/mod/in?${server.sessionQuery()}`, server.init('GET'))
				.then(async (res) => {
					if (res.status === 404) server.hello.needed = true;
					if (!res.ok) return;
					for (const line of JSON.parse(res.text || '[]') as Line[]) {
						if (line.type === 'user') await chatPrompt(line);
						else if (line.type === 'control_request') await controlRequest(line);
					}
				})
				.catch(() => {})
				.finally(() => (polling = false));
		});

		const rows = await $.config.list().catch(() => []);
		const configMode = rows.find((r) => r.key === 'permissionMode')?.value;
		const permissionMode = liveMode ?? (typeof configMode === 'string' ? configMode : undefined);
		reportedMode = permissionMode ?? '';
		emit({ type: 'system', subtype: 'init', session_id: sessionId, model, permissionMode });
		return result;
	});

	on('session.end', async ($, e, next) => {
		const link = server.current();
		if (link && (e.reason === 'clear' || e.reason === 'resume')) {
			server.expectRekey(e.reason);
			for (const task of [...taskAgents.keys()]) unlinkTask(task);
			return next(e);
		}
		if (link) {
			// One request, inside the short bound an exit gets.
			const bye = server.init('POST', server.takeOutbox());
			server.open(null);
			const left = Math.max(0, next.budget.remainingMs - 100);
			await Promise.race([
				(async () => {
					await server.flush((u, i) => $.http.fetch(u, i));
					await $.http.fetch(`${link.url}/mod/bye`, bye).catch(() => {});
				})(),
				$.clock.sleep(left)
			]);
		}
		return next(e);
	});

	on('session.measure', ($, e, next) => {
		if (server.current()) {
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
		if (!server.current() || e.agentId) return yield* next(e);
		await server.rekey(() => $.session.id());
		const sessionId = server.current()?.sessionId ?? '';
		// This request carries every row appended so far.
		injected = [];
		if (effort) {
			if (!effortBase) effortBase = { value: e.effort };
			else if (e.effort !== effortBase.value) effort = effortBase = undefined;
		}
		const stream = next(effort ? { ...e, effort } : e);
		currentMessage = `wbmod-${sessionId.slice(0, 8)}-${++messageSeq}`;
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
						session_id: sessionId,
						message: { id: currentMessage, model, content: [], usage: chunk.usage }
					});
			}
			yield chunk;
		}
		return stream.result;
	});

	on('session.append', async ($, e, next) => {
		const m = e.message;
		if (!server.current() || e.agentId) return next(e);
		await server.rekey(() => $.session.id());
		const sessionId = server.current()?.sessionId;
		if (!sessionId) return next(e);
		if (m.type === 'attachment' && m.name === 'queued_command') {
			// A prompt typed in the terminal while a turn ran, folded into that
			// turn. Only a prompt is framed so (history shows only those too).
			const text = promptText(m.content);
			if (text.startsWith(QUEUED_PREFIX))
				emit({
					type: 'user',
					uuid: e.uuid,
					session_id: sessionId,
					message: { role: 'user', content: text }
				});
		} else if (m.isMeta) {
			return next(e);
		} else if (m.type === 'assistant' && e.door === 'response') {
			emit({
				type: 'assistant',
				uuid: e.uuid,
				session_id: sessionId,
				message: { ...m, id: currentMessage || `wbmod-${e.uuid}`, model }
			});
		} else if (m.type === 'user' && e.door === 'prompt') {
			if (echoed.delete(promptText(m.content))) return next(e);
			emit({ type: 'user', uuid: e.uuid, session_id: sessionId, message: m });
		} else if (m.type === 'user' && e.door === 'tool-result') {
			const row: Line = { type: 'user', uuid: e.uuid, session_id: sessionId, message: m };
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
		if (server.current()) emit({ type: 'prompt_suggestion', suggestion: e.text });
		return shown;
	});

	on('turn.complete', ($, e, next) => {
		const linked = server.current() !== null;
		const task = e.agentId && asyncAgents.has(e.agentId) ? agentTasks.get(e.agentId) : undefined;
		if (linked && task && e.agentId) {
			unlinkTask(task);
			emit({
				type: 'system',
				subtype: 'task_notification',
				task_id: task,
				status: e.reason === 'aborted' ? 'stopped' : e.reason === 'answer' ? 'completed' : 'failed',
				uuid: `wbmod-done-${task}`
			});
		}
		if (linked && !e.agentId) {
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
				failedResult = `wbmod-result-${++askSeq}`;
				emit({
					type: 'result',
					subtype: 'error_during_execution',
					is_error: true,
					result: failure ?? 'The turn failed.',
					uuid: failedResult,
					modelUsage
				});
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
		if (server.current()) {
			failure = e.error_details || `The turn failed: ${e.error.replace(/_/g, ' ')}.`;
			// The turn's `result` went out already: only its notice takes the reason.
			if (failedResult)
				emit({
					type: 'system',
					subtype: 'local_command_output',
					content: failure,
					uuid: failedResult
				});
		}
		return next(e);
	});

	on('classic.PostModelSwitch', ($, e, next) => {
		if (server.current()) {
			model = e.to_model;
			emit({
				type: 'system',
				subtype: 'init',
				model: e.to_model,
				...(PICKED_MODEL.has(e.source) ? { modelChoice: e.requested_model ?? 'default' } : {})
			});
		}
		return next(e);
	});

	// Fires when auto mode's classifier denies a call.
	on('classic.PermissionDenied', ($, e, next) => {
		notePermissionMode(e.permission_mode);
		if (server.current() && !e.agent_id)
			emit({
				type: 'system',
				subtype: 'permission_denied',
				tool_name: e.tool_name,
				tool_use_id: e.tool_use_id,
				decision_reason_type: 'classifier',
				decision_reason: e.reason,
				uuid: `wbmod-denied-${e.tool_use_id}`
			});
		return next(e);
	});

	// An MCP server asks for input. The terminal's dialog answers it; the chat
	// shows a read-only card, and the session waits on it until it's answered.
	on('classic.Elicitation', ($, e, next) => {
		if (server.current()) {
			const id = `wbmod-elicit-${++askSeq}`;
			const key = elicitationKey(e);
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
		const key = elicitationKey(e);
		const id = terminalElicitations.get(key)?.shift();
		if (terminalElicitations.get(key)?.length === 0) terminalElicitations.delete(key);
		if (server.current() && id)
			emit({ type: 'workbench_terminal_elicitation_result', id, action: e.action });
		return next(e);
	});

	// A command that runs no model turn (`/cost`, a forked skill like `/code-review`)
	// prints instead: a chat that sent it waits for that and a `result`, as `-p`
	// prints them. A forked skill's agent has no `agent.spawn`, so it joins the
	// tasks panel here; its own `turn.complete` ends the task.
	on('command.run', async ($, e, next) => {
		if (!server.current()) return next(e);
		await server.rekey(() => $.session.id());
		const before = new Set((await $.agent.list()).map((a) => a.id));
		const result = await next(e);
		if (!server.current()) return result;
		await server.rekey(() => $.session.id());
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
		if (!server.current() || e.agentId || e.trigger === 'precompute') return result;
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

	// A chat's prompt reaches the session through this plugin's own submit;
	// one typed at the terminal hands what the turn asks back to the terminal.
	on('prompt.submit', async ($, e, next) => {
		if (e.origin.kind === 'plugin') chatTurn = e.origin.name === $.plugin.name;
		else if (e.origin.kind === 'composer') chatTurn = false;
		return next(e);
	});

	// Structured results (an Artifact's link) and subagents for the tasks panel.
	on('tool.call', async ($, e, next) => {
		if (!server.current()) return next(e);
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
			const answer = await server.askInChat(
				(u, i) => $.http.fetch(u, i),
				requestId,
				askLine(requestId, e.tool, input, id),
				next.signal,
				chatTurn
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
		const isSkill = !e.agentId && e.tool === 'Skill';
		if (isSkill && id) {
			const before = new Set((await $.agent.list()).map((a) => a.id));
			runningSkills.set(id, {
				skill: (e as unknown as { skill?: string }).skill ?? 'skill',
				before
			});
		}
		const skillTask = runningSkills.size === 1 ? [...runningSkills.keys()][0] : undefined;
		if (e.agentId && skillTask && !taskAgents.has(skillTask) && !agentTasks.has(e.agentId)) {
			const agent = (await $.agent.list()).find((a) => a.id === e.agentId);
			if (agent && !agent.parentId && !runningSkills.get(skillTask)!.before.has(agent.id))
				startSkillTask(skillTask, agent.id);
		}
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
		const launched = result.result as
			| { status?: string; agentId?: string; success?: boolean; background?: boolean }
			| undefined;
		if (isAgent && id) {
			runningAgents.delete(id);
		}
		if (isSkill && id) {
			const forked = launched?.status === 'forked' ? launched.agentId : undefined;
			const guessed = taskAgents.get(id);
			if (guessed && guessed !== forked) unlinkTask(id);
			if (forked && guessed !== forked) startSkillTask(id, forked);
			if (forked && launched?.background) asyncAgents.add(forked);
			else if (taskAgents.has(id) || guessed) {
				unlinkTask(id);
				emit({
					type: 'system',
					subtype: 'task_notification',
					task_id: id,
					status:
						launched?.success === false || result.isError || result.deny ? 'failed' : 'completed',
					uuid: `wbmod-done-${id}`
				});
			}
			runningSkills.delete(id);
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
		if (server.current() && task && result.agentId) {
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
		if (
			verdict.decision === 'ask' &&
			server.current() &&
			e.tool_use_id &&
			!ASKED_IN_CALL.has(e.tool)
		) {
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
	// is open; the server answers `fallback` when none has shown it, and the
	// terminal's dialog asks instead. A held request in flight doesn't spend the
	// hook's time budget, however long the person takes.
	on('classic.PermissionRequest', async ($, e, next) => {
		notePermissionMode(e.permission_mode);
		if (!server.current() || ASKED_IN_CALL.has(e.tool_name)) return next(e);
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
		const answer = await server.askInChat(
			(u, i) => $.http.fetch(u, i),
			requestId,
			line,
			next.signal,
			chatTurn
		);
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
