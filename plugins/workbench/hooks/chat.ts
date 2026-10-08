import type { PermissionRequestDecision, Register, TurnStepInput } from 'claude-code';
import * as server from './link';
import { PendingAsks } from './asks';
import { notifiedJob, startJob, stoppedJobs } from './jobs';
import {
	askLine,
	attachedFiles,
	commandRowOutput,
	midTurn,
	modelOption,
	promptText,
	QUEUED_NUDGE,
	QUEUED_PREFIX,
	rateLimitLine,
	slimResult,
	slashCommand,
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
// Model picked in chat, for this session only: the resolved id each
// main-thread request names, the pick as the chat shows it, the effort levels
// that model takes (absent: unknown), and the model the engine names on its
// own (`base`, from the first request after the pick). A request naming
// another (a fallback) keeps it. A pick in the TUI (`/model`, `/config`) clears it.
let modelPick: { id: string; choice: string; effortLevels?: string[]; base?: string } | undefined;
// The slash commands the chat was last sent, to send a changed list once.
let commandList = '';
// The session title the chat was last sent.
let title: string | undefined;
// The running main-thread turn, for an interrupt from chat.
let runningTurn: string | undefined;
// The live model's context window, from the latest measurement.
let contextWindow: number | undefined;
// The window the chat was last sent outside a `result`; a model switch
// clears the chat's, so it's sent again.
let sentWindow: number | undefined;
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
// Background agents the chat's Stop asked to end. No call stops one, so each ends
// at its next model request or tool call (`endedByStop`), and then its "finished"
// notification starts no turn. One that answered first keeps its notification.
const stoppedAgents = new Set<string>();
const endedByStop = new Set<string>();
// Main-thread Skill calls still running: call id → the skill and the agents
// there were before it. A forked skill's agent has no `agent.spawn`: the first
// call of an agent new since then names it, and its Skill call is its task.
const runningSkills = new Map<string, { skill: string; before: Set<string> }>();
// Whether the latest prompt came from a chat: what it asks waits for a chat's
// answer even before one has it open (a phone in the background).
let chatTurn = false;
// The chat's latest command: whether it started a turn or compaction or got its
// `result` (`answered`), and whether `$.command.run` has resolved (`settled`).
type ChatCommand = { answered: boolean; settled: boolean };
let chatCommand: ChatCommand | undefined;
const PANEL_WAIT_MS = 3000;
const LIVE_AGENT = new Set(['pending', 'running', 'waiting']);
// Chat prompts appended into the running turn that no request has read yet.
let injected: string[] = [];
// Prompts the plugin submitted itself, kept out of the chat (each echoes once).
const echoed = new Set<string>();
// What the latest command printed, through its `text` or its transcript row,
// whichever came first: the other isn't shown again.
const printed = new Set<string>();

// A question is asked from `tool.call` and its answer is the call's result:
// since 2.1.292 a plugin's `allow` doesn't dismiss the dialog of a tool that
// requires the person. A plan is approved in the terminal (PermissionRequest).
const ASKED_IN_CALL = 'AskUserQuestion';
const pendingAsks = new PendingAsks();
// Approvals the terminal's own dialog asks (no chat was open): tool call id →
// request id, and whether the main thread asked it (a background agent's
// outlives the turn).
const askedInTerminal = new Map<string, { requestId: string; main: boolean }>();
// MCP elicitations the terminal shows, by server and elicitation id, oldest first.
const terminalElicitations = new Map<string, string[]>();
// Model switches the person made (`/model`, `/config`, the chat's pick); an
// automatic fallback or a resume keeps the pick the chat shows.
const PICKED_MODEL = new Set(['command', 'picker', 'sdk']);

// Without `model`: the TUI names the bare id, and resending it would drop
// the `[1m]` the chat's model carries.
function sendWindow(window: number | undefined) {
	if (window === undefined || window === sentWindow) return;
	sentWindow = window;
	emit({ type: 'system', subtype: 'init', contextWindow: window });
}

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

// What a command printed, and the agents it started (a forked skill's has no
// `agent.spawn`): each joins the tasks panel, and its own `turn.complete` ends it.
function commandRan(
	command: string,
	args: string,
	text: string | undefined,
	agents: readonly {
		id: string;
		parentId?: string;
		description?: string;
		type?: string;
		status: string;
	}[],
	before: Set<string>
) {
	for (const agent of agents) {
		// A finished one is a turn's that ran while the command waited, not the command's.
		if (before.has(agent.id) || agent.parentId || agentTasks.has(agent.id)) continue;
		if (!LIVE_AGENT.has(agent.status)) continue;
		linkAgent(agent.id, agent.id);
		asyncAgents.add(agent.id);
		emit(
			{
				type: 'system',
				subtype: 'task_started',
				task_id: agent.id,
				description: agent.description || `/${command}`,
				subagent_type: agent.type,
				task_type: 'local_agent',
				is_backgrounded: true,
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
	// Written to the JSONL only: the chat would see it on its next load.
	if (command === 'rename') noteTitle(args);
	if (text !== undefined) printOnce(text);
}

function printOnce(text: string) {
	const key = text.trim();
	if (printed.has(key)) return;
	printed.add(key);
	commandOutput(text);
}

function commandOutput(content: string, inTerminal = false) {
	emit({
		type: 'system',
		subtype: 'local_command_output',
		content,
		...(inTerminal && { workbench_in_terminal: true }),
		uuid: `wbmod-command-${++askSeq}`
	});
}

// The `result` a chat's command that ran no turn ends with, once.
function answerCommand(command: ChatCommand, failed = false) {
	command.answered = true;
	emit(
		failed
			? { type: 'result', subtype: 'error_during_execution', is_error: true }
			: { type: 'result', subtype: 'success', is_error: false }
	);
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
	if (server.current()) emit(...stoppedJobs(tasks));
}

/**
 * The session's title (a `/rename`, or the generated one) as a hook input
 * carries it; the chat folds it as the JSONL's rename.
 */
export function noteTitle(next: string | undefined) {
	const trimmed = next?.trim();
	if (!trimmed || trimmed === title || !server.current()) return;
	title = trimmed;
	emit({ type: 'custom-title', customTitle: trimmed });
}

function commandsLine(commands: readonly { name: string; description: string }[]) {
	const list = commands.map((c) => ({ name: c.name, description: c.description }));
	const text = JSON.stringify(list);
	if (text === commandList) return undefined;
	commandList = text;
	return { type: 'workbench_commands', commands: list };
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
		const fetch = (u: string, i: Parameters<typeof $.http.fetch>[1]) => $.http.fetch(u, i);
		const after: server.After = (ms, fn) => $.clock.after(ms, fn);
		// Bounded: `claude` waits on this hook before its first prompt. A failed
		// hello (the server is down, or another terminal still holds the session)
		// is said again from the timer below, so the session still becomes a chat.
		await server.attach(fetch, false, after);

		// A chat prompt. A plugin's submit waits for the turn to end, so
		// mid-turn it joins the running turn the way a typed prompt would.
		const chatPrompt = async (line: Line) => {
			const message = line.message as { content?: unknown } | undefined;
			const text = promptText(message?.content);
			const files = attachedFiles(line);
			if (!text) return;
			// A plugin's submit refuses a leading `/`, so a known command runs as one
			// (queued until idle); `/tmp is full` stays a prompt. A command that starts
			// no turn (a forked skill, `/rename`) resolves to nothing for the model, so
			// the chat gets its `result` here; a prompt-type one's turn ends with its own.
			const slash = slashCommand(text);
			if (slash && (await $.command.list()).some((c) => c.name === slash.command)) {
				const before = new Set((await $.agent.list()).map((a) => a.id));
				const command: ChatCommand = { answered: false, settled: false };
				chatCommand = command;
				printed.clear();
				// Queued behind a turn typed at the terminal, it isn't that turn's to ask for.
				if (!runningTurn) chatTurn = true;
				const idle = (async () => {
					while (runningTurn) await $.clock.sleep(250);
				})();
				const args = withAttachments(slash.args, files);
				void $.command.run({ command: slash.command, args }).then(
					async (result) => {
						command.settled = true;
						if (!server.current()) return;
						await server.rekey(() => $.session.id());
						commandRan(slash.command, slash.args, result.text, await $.agent.list(), before);
						// Notes for the model may start a turn a moment later (`/goal`),
						// whose `result` ends the command; a `/rename`'s start none.
						for (let i = 0; result.context?.length && i < 4; i++) {
							if (command.answered || runningTurn) break;
							await $.clock.sleep(250);
						}
						if (!command.answered && !runningTurn) answerCommand(command);
					},
					(err: unknown) => {
						command.settled = true;
						if (!server.current()) return;
						commandOutput(`/${slash.command} failed: ${String(err)}`);
						if (!command.answered) answerCommand(command, true);
					}
				);
				// A panel (`/usage`, `/config`) resolves only once it's closed in the TUI,
				// and nothing tells it from a slow command: the chat is told where it is.
				void (async () => {
					await idle;
					await $.clock.sleep(PANEL_WAIT_MS);
					if (command.settled || command.answered || runningTurn || !server.current()) return;
					commandOutput(
						`/${slash.command} opened in the terminal: switch to Terminal to use or close it.`,
						true
					);
					answerCommand(command);
				})();
				return;
			}
			const appended = runningTurn
				? await $.session
						.append({
							message: { type: 'user', content: [{ type: 'text', text: midTurn(text, files) }] }
						})
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
				void $.prompt.submit({ text: withAttachments(text, files), asUser: true });
			}
		};

		const controlRequest = async (line: Line) => {
			const req = (line.request ?? {}) as {
				subtype?: string;
				model?: string;
				resolvedModel?: string;
				effortLevels?: string[];
				settings?: { effortLevel?: string };
			};
			const sub = req.subtype;
			if (sub === 'interrupt') {
				if (runningTurn) await $.turn.abort({ turnId: runningTurn }).catch(() => {});
				for (const agent of await $.agent.list().catch(() => []))
					if (asyncAgents.has(agent.id) && LIVE_AGENT.has(agent.status))
						stoppedAgents.add(agent.id);
				reply(line.request_id);
			} else if (sub === 'initialize') {
				// A fallback list: the server replaces it with the CLI's own.
				const row = (await $.config.list()).find((r) => r.key === 'model');
				const models = (row?.options ?? []).map(modelOption);
				const commands = (await $.command.list()).map((c) => ({
					name: c.name,
					description: c.description
				}));
				commandList = JSON.stringify(commands);
				const modelChoice =
					modelPick?.choice ?? (typeof row?.value === 'string' ? row.value : undefined);
				reply(line.request_id, undefined, { models, commands, modelChoice });
			} else if (sub === 'set_model' && req.model) {
				// `turn.step` takes an id, not an alias: the server resolves the pick.
				if (!req.resolvedModel) {
					reply(line.request_id, `Model: ${req.model} isn't in Claude's model list.`);
					return;
				}
				modelPick = { id: req.resolvedModel, choice: req.model, effortLevels: req.effortLevels };
				model = req.resolvedModel;
				sentWindow = undefined;
				reply(line.request_id);
				emit({ type: 'system', subtype: 'init', model, modelChoice: req.model });
			} else if (sub === 'apply_flag_settings' && req.settings?.effortLevel) {
				effort = req.settings.effortLevel as TurnStepInput['effort'];
				effortBase = undefined;
				reply(line.request_id);
			} else {
				reply(line.request_id, `${sub ?? 'This'} isn't available in a terminal session yet.`);
			}
		};

		$.clock.every(50, () => {
			server.tick();
			if (!server.isFlushing()) void server.flush(fetch);
		});
		$.clock.every(300, () => {
			if (polling || !server.current()) return;
			polling = true;
			void (async () => {
				await server.rekey(() => $.session.id());
				if (server.hello.needed) {
					if (Date.now() - server.hello.last < server.HELLO_EVERY_MS) return;
					if (await server.attach(fetch, true, after)) await reattached();
					return;
				}
				for (const line of await server.poll(fetch)) {
					try {
						if (line.type === 'user') await chatPrompt(line);
						else if (line.type === 'control_request') await controlRequest(line);
					} catch {
						// Not run again: a line that fails would fail every time.
					} finally {
						server.handled(line);
					}
				}
			})()
				.catch(() => {})
				.finally(() => (polling = false));
		});

		// The server lost the session and loaded it again from its file: what it
		// was told outside the history is told again.
		const reattached = async () => {
			lastSettings = '';
			reportedMode = '';
			sentWindow = undefined;
			notePermissionMode(liveMode);
			if (title) emit({ type: 'custom-title', customTitle: title });
			// A restarted server has no plan usage reading until a window moves.
			const usage = await $.session.usage().catch(() => undefined);
			const limits = usage && rateLimitLine(usage.rateLimits);
			if (limits) emit(limits);
			sendWindow(contextWindow ?? usage?.context.window);
		};

		// No hook has named the live mode yet: the one Workbench started `claude`
		// in beats the settings default.
		liveMode ??= (await $.env.get('WORKBENCH_PERMISSION_MODE')) || undefined;
		const rows = await $.config.list().catch(() => []);
		const configMode = rows.find((r) => r.key === 'permissionMode')?.value;
		const permissionMode = liveMode ?? (typeof configMode === 'string' ? configMode : undefined);
		reportedMode = permissionMode ?? '';
		emit({ type: 'system', subtype: 'init', session_id: sessionId, model, permissionMode });
		const usage = await $.session.usage().catch(() => undefined);
		const limits = usage && rateLimitLine(usage.rateLimits);
		if (limits) emit(limits);
		// A measurement taken meanwhile is newer.
		contextWindow ??= usage?.context.window;
		sentWindow = undefined;
		sendWindow(contextWindow);
		return result;
	});

	on('session.end', async ($, e, next) => {
		const link = server.current();
		if (link && (e.reason === 'clear' || e.reason === 'resume')) {
			server.expectRekey(e.reason);
			for (const task of [...taskAgents.keys()]) unlinkTask(task);
			stoppedAgents.clear();
			endedByStop.clear();
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
			sendWindow(contextWindow);
			const limit = e.changed.includes('rateLimits') ? rateLimitLine(e.rateLimits) : undefined;
			if (limit) emit(limit);
		}
		return next(e);
	});

	// A subagent's run raises none: this is always the main thread's turn.
	// Commands come and go (a plugin or skill loaded), so the chat's list is
	// re-read here.
	on('turn.start', async ($, e, next) => {
		runningTurn = e.turnId;
		// A prompt-type command's turn ends with its own `result`.
		if (chatCommand) chatCommand.answered = true;
		failure = undefined;
		failedResult = undefined;
		const result = await next(e);
		const commands = server.current() ? commandsLine(await $.command.list()) : undefined;
		if (commands) emit(commands);
		return result;
	});

	on('turn.step', async function* ($, e, next) {
		if (e.agentId && stoppedAgents.has(e.agentId)) {
			endedByStop.add(e.agentId);
			return {
				turnId: e.turnId,
				index: e.index,
				answer: '',
				toolUses: [],
				stopReason: 'end_turn',
				usage: null
			};
		}
		if (!server.current() || e.agentId) return yield* next(e);
		await server.rekey(() => $.session.id());
		const sessionId = server.current()?.sessionId ?? '';
		// This request carries every row appended so far.
		injected = [];
		// The TUI picked another effort: the chat's is dropped (and the server forgets it).
		let effortCleared = false;
		if (effort) {
			if (!effortBase) effortBase = { value: e.effort };
			else if (e.effort !== effortBase.value) {
				effort = effortBase = undefined;
				effortCleared = true;
			}
		}
		if (modelPick && modelPick.base === undefined) modelPick.base = e.model;
		const pick = modelPick && e.model === modelPick.base ? modelPick : undefined;
		// What this request really runs with: the engine resolved both. A picked
		// model that doesn't take the effort goes without.
		let sent = effort ?? e.effort;
		if (pick?.effortLevels && !(typeof sent === 'string' && pick.effortLevels.includes(sent)))
			sent = undefined;
		const step = { ...e, ...(pick ? { model: pick.id } : {}) };
		if (sent !== e.effort) step.effort = sent;
		const stream = next(step);
		currentMessage = `wbmod-${sessionId.slice(0, 8)}-${++messageSeq}`;
		startedBlocks.clear();
		model = step.model || model;
		const settings = `${model} ${sent ?? ''}`;
		if (settings !== lastSettings) {
			lastSettings = settings;
			emit({
				type: 'system',
				subtype: 'init',
				model,
				// `null`: this model runs without effort.
				effort: typeof sent === 'string' ? sent : null,
				...(effortCleared ? { effortCleared } : {})
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
		if (m.type === 'attachment' && m.name === 'goal_status') {
			// The hook's view has none of its fields: the server reads the row
			// from the session file, written once `next` returns.
			const result = await next(e);
			emit({ type: 'workbench_goal_status', uuid: e.uuid });
			return result;
		}
		if (e.door === 'command') {
			// A command's own rows: its name, the chat's echo of a command it sent,
			// and what it printed, which `$.command.run` and the `command.run` hook
			// may not return as `text` (`/goal`, `/rename`).
			const text = promptText(m.content);
			const out = commandRowOutput(text);
			if (text.includes('<command-name>'))
				emit({
					type: 'user',
					uuid: e.uuid,
					session_id: sessionId,
					message: { role: 'user', content: text }
				});
			else if (out) printOnce(out);
			return next(e);
		}
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
		if (e.agentId) stoppedAgents.delete(e.agentId);
		if (linked && task && e.agentId) {
			unlinkTask(task);
			emit({
				type: 'system',
				subtype: 'task_notification',
				task_id: task,
				status:
					e.reason === 'aborted' || endedByStop.has(e.agentId)
						? 'stopped'
						: e.reason === 'answer'
							? 'completed'
							: 'failed',
				uuid: `wbmod-done-${task}`
			});
		}
		// A call denied in the terminal's dialog never reaches `tool.call`.
		if (!e.agentId)
			for (const [id, asked] of askedInTerminal) if (asked.main) askedInTerminal.delete(id);
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
		// The later pick wins, as it does in the TUI.
		if (PICKED_MODEL.has(e.source)) modelPick = undefined;
		if (server.current()) {
			model = e.to_model;
			sentWindow = undefined;
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
		pendingAsks.drop(e.tool_use_id);
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

	// A command typed at the terminal. A chat's runs through `$.command.run`, which
	// skips this plugin's own hook, so `chatPrompt` does the same itself.
	on('command.run', async ($, e, next) => {
		if (e.origin.kind === 'composer') chatTurn = false;
		if (!server.current()) return next(e);
		await server.rekey(() => $.session.id());
		printed.clear();
		const before = new Set((await $.agent.list()).map((a) => a.id));
		const result = await next(e);
		if (!server.current()) return result;
		await server.rekey(() => $.session.id());
		commandRan(e.command, e.args, result.text, await $.agent.list(), before);
		return result;
	});

	// `/compact` (the chat's, or the server's upkeep) runs no turn either: the chat
	// needs the boundary and a `result`. Auto compaction runs inside a turn, which
	// ends with its own.
	on('session.compact', async ($, e, next) => {
		const ours = !e.agentId && e.trigger !== 'precompute';
		// The chat's `/compact` running: not a panel waiting to be closed, however long it takes.
		if (ours && e.trigger === 'manual' && chatCommand) chatCommand.answered = true;
		const result = await next(e);
		if (!server.current() || !ours) return result;
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
		const tasks =
			e.origin.kind === 'task-notification'
				? [...e.text.matchAll(/<task-id>([^<]+)<\/task-id>/g)].map((m) => m[1] ?? '')
				: [];
		if (tasks.length && tasks.every((t) => endedByStop.has(t))) {
			for (const t of tasks) endedByStop.delete(t);
			return { drop: 'Stopped in Workbench chat' };
		}
		if (e.origin.kind === 'plugin') chatTurn = e.origin.name === $.plugin.name;
		else if (e.origin.kind === 'composer') chatTurn = false;
		return next(e);
	});

	// A background job's notification names how it ended (a killed one too);
	// the Stop hook's job list can only tell running from gone.
	on('session.receive', ($, e, next) => {
		const line =
			server.current() && !e.agentId && e.origin.kind === 'task-notification'
				? notifiedJob(e.text)
				: undefined;
		if (line) emit(line);
		return next(e);
	});

	// Structured results (an Artifact's link) and subagents for the tasks panel.
	on('tool.call', async ($, e, next) => {
		if (e.agentId && stoppedAgents.has(e.agentId)) return { deny: 'Stopped in Workbench chat' };
		if (!server.current()) return next(e);
		const id = e.tool_use_id;
		// Approved in the terminal's dialog: it no longer waits, however long the call runs.
		// The call goes ahead: whatever settled it, it no longer waits on a dialog.
		if (id) pendingAsks.drop(id);
		const asked = id ? askedInTerminal.get(id) : undefined;
		if (id && asked) {
			askedInTerminal.delete(id);
			emit({ type: 'control_cancel_request', request_id: asked.requestId });
		}
		if (e.tool === ASKED_IN_CALL && !e.agentId && id) {
			const { tool: _tool, tool_use_id: _id, agentId: _agent, ...input } = e;
			const requestId = `wbmod-ask-${++askSeq}`;
			const answer = await server.askInChat(
				(u, i) => $.http.fetch(u, i),
				(ms, fn) => $.clock.after(ms, fn),
				requestId,
				askLine(requestId, e.tool, input, id),
				next.signal,
				chatTurn
			);
			if (answer) {
				if (answer.behavior !== 'allow')
					return { deny: answer.message || 'Declined in Workbench chat' };
				const answered = { ...e, ...(answer.updatedInput ?? {}) } as typeof e;
				return {
					result: {
						questions: answered.questions,
						answers: answered.answers ?? {},
						...(answered.annotations ? { annotations: answered.annotations } : {})
					}
				};
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
		let result: Awaited<ReturnType<typeof next>>;
		try {
			result = await next(e);
		} catch (err) {
			// No result comes: what the call holds goes, or the one-running-call
			// guesses above would pin later progress on it.
			if (id && (isAgent || (isSkill && taskAgents.has(id)))) {
				unlinkTask(id);
				emit({
					type: 'system',
					subtype: 'task_notification',
					task_id: id,
					status: 'failed',
					uuid: `wbmod-done-${id}`
				});
			}
			if (id) {
				runningAgents.delete(id);
				runningSkills.delete(id);
			}
			if (main) {
				callsRunning.delete(main);
				toolRows.delete(main);
			}
			throw err;
		}
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
		if (e.tool === 'Bash' && bg && startJob(bg)) {
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
		if (
			verdict.decision === 'ask' &&
			server.current() &&
			e.tool_use_id &&
			e.tool !== ASKED_IN_CALL
		) {
			pendingAsks.note(e.tool, e.input, { id: e.tool_use_id, reason: verdict.reason });
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
		if (!server.current() || e.tool_name === ASKED_IN_CALL) return next(e);
		// Approving a plan is what leaves plan mode, and since 2.1.292 only the
		// dialog does that (a hook's allow doesn't dismiss it): it stays the
		// terminal's, and the chat says where to answer it.
		const pending = pendingAsks.take(e.tool_name, e.tool_input);
		if (e.tool_name === 'ExitPlanMode') {
			const requestId = `wbmod-ask-${++askSeq}`;
			emit({
				type: 'system',
				subtype: 'local_command_output',
				content: 'Claude has a plan for you to review. Approve it in the terminal.',
				workbench_in_terminal: true,
				uuid: `wbmod-plan-${requestId}`
			});
			await server.askInTerminal(
				(u, i) => $.http.fetch(u, i),
				(ms, fn) => $.clock.after(ms, fn),
				requestId,
				askLine(requestId, e.tool_name, e.tool_input, pending?.id)
			);
			if (pending) askedInTerminal.set(pending.id, { requestId, main: !e.agent_id });
			return next(e);
		}
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
			(ms, fn) => $.clock.after(ms, fn),
			requestId,
			line,
			next.signal,
			chatTurn
		);
		if (!answer) {
			// The terminal asks now; the server shows it waiting until it's answered.
			if (pending && !next.signal.aborted)
				askedInTerminal.set(pending.id, { requestId, main: !e.agent_id });
			return next(e);
		}
		const decision: PermissionRequestDecision =
			answer.behavior === 'allow'
				? { behavior: 'allow', updatedPermissions: answer.updatedPermissions }
				: { behavior: 'deny', message: answer.message || 'Denied in Workbench chat' };
		return { decision };
	});
};
