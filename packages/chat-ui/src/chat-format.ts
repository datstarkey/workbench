import { replaceToken, tokenAtCaret, type CaretToken } from './composer-tokens';
import type { ElicitationItem } from './elicitation-form';
import { isArtifactTool } from './artifacts';
import { contextUsage } from './usage-format';
import type {
	AgentKind,
	ClaudeAccount,
	CodexMode,
	EffortLevel,
	ModelOption,
	PermissionMode,
	RateLimitInfo,
	RetryInfo,
	SlashCommand,
	TaskInfo,
	TranscriptItem,
	TranscriptMeta,
	TranscriptPatchHunk
} from '@workbench/types';

export type ToolItem = Extract<TranscriptItem, { kind: 'tool' }>;
export type ApprovalItem = Extract<TranscriptItem, { kind: 'approval' }>;

/** Tools that only look around. Runs of these collapse into one row of chips. */
const QUIET_TOOLS = new Set(['Read', 'Grep', 'Glob', 'LS', 'ToolSearch']);

/** Tools the plan panel shows (`latestTodos`) rather than the transcript. */
const PLAN_TOOLS = new Set(['TodoWrite', 'TaskCreate', 'TaskUpdate', 'TaskList']);

export type StepBlock =
	| { kind: 'item'; item: TranscriptItem }
	| { kind: 'quiet'; id: string; tools: ToolItem[] };

export type ChatBlock =
	| StepBlock
	/** Finished tool calls (and the thinking between them), folded into one row. */
	| { kind: 'steps'; id: string; blocks: StepBlock[]; tools: ToolItem[] };

/**
 * Apply an update's `[index, item]` changes: replace by id, append new ones.
 * Indices below `start` belong to history the snapshot left out; placing them
 * would put an old item at the bottom of the chat.
 */
export function applyChanges(
	items: TranscriptItem[],
	start: number,
	changes: [number, TranscriptItem][]
): TranscriptItem[] {
	const next = items.slice();
	const at = new Map(next.map((item, i) => [item.id, i]));
	for (const [index, item] of changes) {
		if (index < start) continue;
		const i = at.get(item.id);
		if (i === undefined) {
			at.set(item.id, next.length);
			next.push(item);
		} else {
			next[i] = item;
		}
	}
	return next;
}

/** Group consecutive read-only tool calls so a turn reads as prose, not a log. */
export function groupBlocks(items: TranscriptItem[]): ChatBlock[] {
	const blocks: StepBlock[] = [];
	for (const item of items) {
		const last = blocks[blocks.length - 1];
		if (item.kind === 'tool' && QUIET_TOOLS.has(item.name) && item.status !== 'error') {
			if (last?.kind === 'quiet') last.tools.push(item);
			else blocks.push({ kind: 'quiet', id: item.id, tools: [item] });
		} else if (item.kind !== 'tool' || !PLAN_TOOLS.has(item.name) || item.status === 'error') {
			blocks.push({ kind: 'item', item });
		}
	}
	return foldSteps(blocks);
}

function stepTools(block: StepBlock): ToolItem[] | null {
	if (block.kind === 'quiet') return block.tools;
	if (block.item.kind === 'thinking') return [];
	// An artifact card carries a link, so it stays in view.
	return block.item.kind === 'tool' &&
		block.item.status === 'ok' &&
		!isArtifactTool(block.item.name)
		? [block.item]
		: null;
}

/** Runs with two or more finished calls fold; running and failed calls stay in view. */
function foldSteps(blocks: StepBlock[]): ChatBlock[] {
	const out: ChatBlock[] = [];
	let run: StepBlock[] = [];
	let tools: ToolItem[] = [];
	const flush = () => {
		if (tools.length >= 2) {
			const first = run[0];
			out.push({
				kind: 'steps',
				id: first.kind === 'quiet' ? first.id : first.item.id,
				blocks: run,
				tools
			});
		} else out.push(...run);
		run = [];
		tools = [];
	};
	for (const block of blocks) {
		const found = stepTools(block);
		if (found === null) {
			flush();
			out.push(block);
		} else {
			run.push(block);
			tools.push(...found);
		}
	}
	flush();
	return out;
}

/** `Bash, Read, Edit` — the tools in a folded run, in first-use order. */
export function stepNames(tools: ToolItem[]): string {
	return [...new Set(tools.map((t) => t.name))].join(', ');
}

function str(input: ToolItem['input'], key: string): string {
	const v = input?.[key];
	return typeof v === 'string' ? v : '';
}

/** Path relative to the session cwd when inside it (either separator). */
export function shortPath(path: string, cwd?: string): string {
	if (!cwd) return path;
	const base = cwd.replace(/[\\/]+$/, '');
	if (path.startsWith(base) && /[\\/]/.test(path.charAt(base.length))) {
		return path.slice(base.length + 1);
	}
	return path;
}

/** One-line description of what a tool call did. */
export function toolDetail(tool: ToolItem, cwd?: string): string {
	const input = tool.input;
	switch (tool.name) {
		case 'Bash':
			return str(input, 'command').split('\n')[0];
		case 'Read':
		case 'Edit':
		case 'MultiEdit':
		case 'Write':
		case 'NotebookEdit':
			return shortPath(str(input, 'file_path') || str(input, 'notebook_path'), cwd);
		case 'Grep':
		case 'Glob':
			return str(input, 'pattern');
		case 'WebFetch':
			return str(input, 'url');
		case 'WebSearch':
			return str(input, 'query');
		case 'Task':
		case 'Agent':
			return str(input, 'description');
		case 'Skill':
			return str(input, 'skill');
		default: {
			const first = Object.values(input ?? {}).find((v) => typeof v === 'string');
			return typeof first === 'string' ? first.split('\n')[0] : '';
		}
	}
}

/** Tools whose output is Markdown: a skill's body, a subagent's report. */
const MARKDOWN_OUTPUT = new Set(['Skill', 'Agent', 'Task']);

/** Tool output to render as Markdown, or null for plain text. */
export function outputMarkdown(tool: ToolItem, output: string): string | null {
	const json = jsonAsMarkdown(output);
	if (json !== output) return json;
	return MARKDOWN_OUTPUT.has(tool.name) ? output : null;
}

/** Text that is only JSON, fenced as-is (never re-serialised: that loses big numbers and key order). */
export function jsonAsMarkdown(text: string): string {
	const trimmed = text.trim();
	if (!/^[[{]/.test(trimmed)) return text;
	try {
		JSON.parse(trimmed);
	} catch {
		return text;
	}
	const longest = Math.max(0, ...(trimmed.match(/`+/g) ?? []).map((run) => run.length));
	const fence = '`'.repeat(Math.max(3, longest + 1));
	return `${fence}json\n${trimmed}\n${fence}`;
}

export function patchStats(patch: TranscriptPatchHunk[] | undefined): {
	added: number;
	removed: number;
} {
	let added = 0;
	let removed = 0;
	for (const hunk of patch ?? []) {
		for (const line of hunk.lines) {
			if (line.startsWith('+')) added += 1;
			else if (line.startsWith('-')) removed += 1;
		}
	}
	return { added, removed };
}

export interface TodoStep {
	content: string;
	status: 'pending' | 'in_progress' | 'completed';
	activeForm?: string;
}

const STEP_STATUSES = new Set<string>(['pending', 'in_progress', 'completed']);

const optionalString = (v: unknown) => (typeof v === 'string' ? v : undefined);

/**
 * The plan as of the last call: TodoWrite and a TaskList result replace it,
 * TaskCreate and TaskUpdate edit it. A task counts once its TaskCreate has a
 * result (an interrupted one never made it), whose text carries the id; items
 * hold only that text, so if its wording changes the next number is assumed
 * (the CLI numbers tasks 1, 2, …).
 */
export function latestTodos(items: TranscriptItem[]): TodoStep[] {
	let plan = new Map<string, TodoStep>();
	let next = 1;
	const seen = (id: string) => {
		if (/^\d+$/.test(id)) next = Math.max(next, Number(id) + 1);
	};
	for (const item of items) {
		// A null input is still streaming.
		if (item.kind !== 'tool' || item.status === 'error' || !item.input) continue;
		const input = item.input;
		if (item.name === 'TodoWrite') {
			if (Array.isArray(input.todos))
				plan = new Map((input.todos as TodoStep[]).map((t, i) => [`todo:${i}`, t]));
		} else if (item.name === 'TaskCreate' && item.output != null) {
			const id = /^Task #(\S+) created successfully/.exec(item.output)?.[1] ?? String(next);
			plan.set(id, {
				content: String(input.subject ?? ''),
				status: 'pending',
				activeForm: optionalString(input.activeForm)
			});
			seen(id);
		} else if (item.name === 'TaskUpdate') {
			const id = String(input.taskId ?? '');
			const step = plan.get(id);
			if (!step) continue;
			if (input.status === 'deleted') plan.delete(id);
			else
				plan.set(id, {
					content: optionalString(input.subject) ?? step.content,
					status: STEP_STATUSES.has(input.status as string)
						? (input.status as TodoStep['status'])
						: step.status,
					activeForm: optionalString(input.activeForm) ?? step.activeForm
				});
		} else if (item.name === 'TaskList' && item.output != null && !item.fullOutputBytes) {
			plan = taskListPlan(item.output, plan);
			for (const id of plan.keys()) seen(id);
		}
	}
	return [...plan.values()];
}

/**
 * TaskList prints `#<id> [<status>] <subject>[ (owner)][ [blocked by #…]]` per
 * task. The owner can't be told from a subject ending in parentheses, so a
 * known subject the line starts with is kept as is.
 */
function taskListPlan(output: string, known: Map<string, TodoStep>): Map<string, TodoStep> {
	const plan = new Map<string, TodoStep>();
	for (const line of output.split('\n')) {
		const m = /^#(\S+) \[(\w+)\] (.*)$/.exec(line);
		if (!m || !STEP_STATUSES.has(m[2])) continue;
		const [, id, status, rest] = m;
		const listed = rest.replace(/ \[blocked by [^\]]*\]$/, '');
		const step = known.get(id);
		plan.set(id, {
			content: step && listed.startsWith(step.content) ? step.content : listed,
			status: status as TodoStep['status'],
			activeForm: step?.activeForm
		});
	}
	return plan;
}

export interface QuestionOption {
	label: string;
	description: string;
	preview?: string;
}

export interface Question {
	id?: string;
	isSecret?: boolean;
	isOther?: boolean;
	question: string;
	header: string;
	multiSelect: boolean;
	options: QuestionOption[];
}

/** The questions of an `AskUserQuestion` call; malformed entries are dropped. */
export function parseQuestions(input: Record<string, unknown> | null): Question[] {
	const raw = input?.questions;
	if (!Array.isArray(raw)) return [];
	return raw.flatMap((q): Question[] => {
		if (typeof q?.question !== 'string' || !Array.isArray(q.options)) return [];
		const options = (q.options as unknown[]).flatMap((o): QuestionOption[] => {
			const opt = o as Partial<QuestionOption> | null;
			return typeof opt?.label === 'string'
				? [
						{
							label: opt.label,
							description: typeof opt.description === 'string' ? opt.description : '',
							...(typeof opt.preview === 'string' ? { preview: opt.preview } : {})
						}
					]
				: [];
		});
		return [
			{
				question: q.question,
				...(typeof q.id === 'string' ? { id: q.id } : {}),
				...(typeof q.isSecret === 'boolean' ? { isSecret: q.isSecret } : {}),
				...(typeof q.isOther === 'boolean' ? { isOther: q.isOther } : {}),
				header: typeof q.header === 'string' ? q.header : '',
				multiSelect: q.multiSelect === true,
				options
			}
		];
	});
}

/** One question's answer as the tool expects it: labels comma-joined, plus own words. */
export function answerFor(selected: string[], other: string): string {
	return [...selected, other.trim()].filter(Boolean).join(', ');
}

/** "Claude" / "Codex", for labels that name the agent. */
export function agentName(agent: AgentKind = 'claude'): string {
	return agent === 'codex' ? 'Codex' : 'Claude';
}

/** What the agent is doing right now, for the activity line under the chat. */
export type Activity =
	| { kind: 'idle' }
	| { kind: 'retrying'; retry: RetryInfo }
	| { kind: 'approval'; approval: ApprovalItem }
	| { kind: 'elicitation'; elicitation: ElicitationItem }
	| { kind: 'tool'; tool: ToolItem }
	| { kind: 'writing' }
	| { kind: 'thinking' };

/** The chat's title, else its first prompt, else the agent's name. */
export function chatTitle(
	items: TranscriptItem[],
	meta: TranscriptMeta | null,
	agent: AgentKind
): string {
	const prompt = items.find((i) => i.kind === 'user' && i.text !== '');
	return meta?.title || (prompt?.kind === 'user' ? prompt.text : '') || agentName(agent);
}

/** An approval, question or MCP elicitation still waiting on the person. */
export function awaitsAnswer(item: TranscriptItem): item is ApprovalItem | ElicitationItem {
	if (item.kind === 'approval')
		return !item.decision && !item.expired && item.input?.isBlocking !== false;
	return item.kind === 'elicitation' && !item.action && !item.expired;
}

export function activity(items: TranscriptItem[], meta: TranscriptMeta | null): Activity {
	for (let i = items.length - 1; i >= 0; i--) {
		const item = items[i];
		if (awaitsAnswer(item)) {
			return item.kind === 'approval'
				? { kind: 'approval', approval: item }
				: { kind: 'elicitation', elicitation: item };
		}
	}
	if (!meta?.busy) return { kind: 'idle' };
	if (meta.retry) return { kind: 'retrying', retry: meta.retry };
	const last = items[items.length - 1];
	if (last?.kind === 'tool' && last.status === 'running') return { kind: 'tool', tool: last };
	if (last?.kind === 'text' && last.text) return { kind: 'writing' };
	return { kind: 'thinking' };
}

/** The part of a tool call a person needs to judge it: the command, the file, or the input. */
export function approvalPreview(item: ApprovalItem, cwd?: string): string {
	const input = item.input ?? {};
	const pick = (key: string) => (typeof input[key] === 'string' ? (input[key] as string) : '');
	return (
		pick('command') ||
		shortPath(pick('file_path') || pick('notebook_path'), cwd) ||
		pick('url') ||
		pick('pattern') ||
		JSON.stringify(input, null, 2)
	);
}

export interface ModeOption {
	mode: PermissionMode | CodexMode;
	label: string;
	hint: string;
	/** Nothing asks first; shown in the error colour. */
	risky?: boolean;
}

/** Modes offered in the picker, in Shift+Tab order. */
export const MODE_OPTIONS: ModeOption[] = [
	{ mode: 'default', label: 'Ask first', hint: 'Claude asks before edits and commands' },
	{ mode: 'acceptEdits', label: 'Accept edits', hint: 'File edits run; commands still ask' },
	{ mode: 'plan', label: 'Plan', hint: 'Claude reads and plans, changes nothing' },
	{ mode: 'auto', label: 'Auto', hint: 'A safety check approves routine actions' },
	{ mode: 'bypassPermissions', label: 'Bypass', hint: 'Nothing asks first', risky: true }
];

/** Codex's `/approvals` presets. */
export const CODEX_MODE_OPTIONS: ModeOption[] = [
	{ mode: 'read-only', label: 'Read only', hint: 'Codex reads; edits and commands ask' },
	{
		mode: 'auto',
		label: 'Auto',
		hint: 'Works freely in the workspace; asks before going outside it'
	},
	{
		mode: 'full-access',
		label: 'Full access',
		hint: 'No sandbox, nothing asks first',
		risky: true
	}
];

export function modeOptions(agent: AgentKind = 'claude'): ModeOption[] {
	return agent === 'codex' ? CODEX_MODE_OPTIONS : MODE_OPTIONS;
}

/** The picker's label; a Codex chat with no mode set follows Codex's own config. */
export function modeLabel(
	mode: PermissionMode | CodexMode | null | undefined,
	agent: AgentKind = 'claude'
): string {
	if (agent === 'codex') {
		return CODEX_MODE_OPTIONS.find((m) => m.mode === mode)?.label ?? 'Codex config';
	}
	if (mode === 'dontAsk') return "Don't ask";
	return MODE_OPTIONS.find((m) => m.mode === mode)?.label ?? 'Ask first';
}

export function isRiskyMode(mode: PermissionMode | CodexMode | null | undefined): boolean {
	return mode === 'bypassPermissions' || mode === 'full-access';
}

/** `m:ss` for the turn timer. */
export function formatElapsed(ms: number): string {
	const total = Math.max(0, Math.floor(ms / 1000));
	return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

const RUNNING: TaskInfo['status'][] = ['pending', 'running', 'paused'];

export function isRunning(task: TaskInfo): boolean {
	return RUNNING.includes(task.status);
}

export type TaskTab = 'agents' | 'jobs';

/**
 * What the tasks panel shows: the picked tab unless it's empty (then agents, then
 * jobs), and the picked task unless it's gone (then the tab's first). Lists keep
 * start order so rows and the default pick don't move as tasks finish.
 */
export function pickTasks(
	tasks: TaskInfo[],
	pickedTab: TaskTab | null,
	pickedId: string | null
): { agents: TaskInfo[]; jobs: TaskInfo[]; tab: TaskTab; selected: TaskInfo | null } {
	const agents = tasks.filter((t) => t.kind === 'agent');
	const jobs = tasks.filter((t) => t.kind !== 'agent');
	const groups = { agents, jobs };
	const tab =
		pickedTab && groups[pickedTab].length > 0 ? pickedTab : agents.length > 0 ? 'agents' : 'jobs';
	const list = groups[tab];
	return { agents, jobs, tab, selected: list.find((t) => t.id === pickedId) ?? list[0] ?? null };
}

export type TaskView = 'conversation' | 'output';

/** A subagent opens on its own conversation; other tasks only have output. */
export function taskViews(task: TaskInfo): TaskView[] {
	return task.kind === 'agent' ? ['conversation', 'output'] : ['output'];
}

export function pickTaskView(task: TaskInfo, picked: TaskView | null): TaskView {
	const views = taskViews(task);
	return picked && views.includes(picked) ? picked : views[0];
}

/** `1.8k tokens`, `24k tokens`. */
export function formatCount(n: number, unit: string): string {
	const value =
		n >= 10_000 ? `${Math.round(n / 1000)}k` : n >= 1000 ? `${(n / 1000).toFixed(1)}k` : `${n}`;
	return `${value} ${unit}`;
}

const LIMIT_NAMES: Record<string, string> = {
	five_hour: '5-hour',
	seven_day: 'weekly',
	seven_day_opus: 'weekly Opus',
	seven_day_sonnet: 'weekly Sonnet'
};

const clockTime = (unixSeconds: number) =>
	new Date(unixSeconds * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });

/** What to tell the person about their usage limit, if anything. */
export function limitNotice(
	info: RateLimitInfo | null,
	formatTime: (unixSeconds: number) => string = clockTime
): { tone: 'warn' | 'blocked'; text: string } | null {
	if (!info || info.status === 'allowed') return null;
	const name = info.kind ? `${LIMIT_NAMES[info.kind] ?? info.kind.replace(/_/g, ' ')} ` : '';
	const resets = info.resetsAt ? ` It resets at ${formatTime(info.resetsAt)}.` : '';
	if (info.status === 'rejected') {
		return { tone: 'blocked', text: `You've reached your ${name}usage limit.${resets}` };
	}
	const used = info.utilization != null ? `${Math.round(info.utilization * 100)}% of ` : 'most of ';
	return { tone: 'warn', text: `You've used ${used}your ${name}usage limit.${resets}` };
}

/** The Claude accounts a chat can run under: the default (`~/.claude`), then each extra one. */
export function accountChoices(
	accounts: Pick<ClaudeAccount, 'id' | 'name'>[]
): { key: string; id: string | undefined; name: string }[] {
	return [
		{ key: 'default', id: undefined, name: 'Default' },
		...accounts.map(({ id, name }) => ({ key: id, id, name }))
	];
}

/** `38 KB`, `1.2 MB`. */
export function formatBytes(n: number): string {
	return n >= 1024 * 1024 ? `${(n / 1024 / 1024).toFixed(1)} MB` : `${Math.ceil(n / 1024)} KB`;
}

/**
 * How full the context window is, 0–1. Both CLIs report their window; while
 * Claude's isn't known (before a turn ends on the current model), `[1m]` on
 * the model means 1M, else 200k.
 */
export function contextUsed(meta: TranscriptMeta | null): number {
	return (contextUsage(meta)?.percent ?? 0) / 100;
}

/** The model the session is on: the one picked here, else matched by id, else the default. */
export function currentModel(meta: TranscriptMeta | null): ModelOption | null {
	if (!meta || meta.models.length === 0) return null;
	const running = meta.model?.replace(/\[1m\]$/, '');
	return (
		meta.models.find((m) => m.value === meta.modelChoice) ??
		meta.models.find((m) => m.resolvedModel === running && m.value !== 'default') ??
		meta.models.find((m) => m.value === 'default') ??
		null
	);
}

/**
 * Whether a model can be picked: the server switches a Claude session by the
 * id the CLI's own list resolves, which the plugin's stand-in list lacks
 * until that list is pinned. The current model stays shown as picked.
 */
export function canPickModel(option: ModelOption, current: ModelOption | null): boolean {
	return option.resolvedModel != null || option.value === current?.value;
}

const EFFORT_LABELS: Record<EffortLevel, string> = {
	none: 'No reasoning',
	minimal: 'Minimal',
	low: 'Low',
	medium: 'Medium',
	high: 'High',
	xhigh: 'Extra high',
	max: 'Max'
};

export function effortLabel(level: EffortLevel | null): string {
	return level ? EFFORT_LABELS[level] : 'Default effort';
}

/** A `/` command being typed: where its `/` is and the name after it, lowercased. */
export type SlashQuery = CaretToken;

/**
 * The `/name` word ending at the caret, at the start or after whitespace, as
 * Claude Code's prompt offers commands mid-line; a path like `src/x` isn't one.
 */
export function slashQuery(draft: string, caret = draft.length): SlashQuery | null {
	const token = tokenAtCaret(draft, caret, '/');
	return token && { ...token, query: token.query.toLowerCase() };
}

/** Only whitespace around the `/name` being typed: it can run or send on its own. */
export function isWholeCommand(draft: string, slash: SlashQuery, caret: number): boolean {
	return !draft.slice(0, slash.start).trim() && !draft.slice(caret).replace(/^\S*/, '').trim();
}

/** The draft with the `/name` being typed replaced by `/<name> `, and the caret after it. */
export function insertCommand(
	draft: string,
	slash: SlashQuery,
	caret: number,
	name: string
): { text: string; caret: number } {
	return replaceToken(draft, slash.start, caret, `/${name} `);
}

/** Commands for the `/` menu: name prefix matches, then name, then description matches. */
export function matchCommands(all: SlashCommand[], query: string): SlashCommand[] {
	// The menu is keyed by name, and a duplicate key stops Svelte rendering the
	// rest of the chat. Which copy Claude Code runs is unknown: the first stays.
	const seen = new Set<string>();
	const commands = all.filter((c) => !seen.has(c.name) && seen.add(c.name));
	if (!query) return commands;
	const rank = (c: SlashCommand) => {
		const name = c.name.toLowerCase();
		if (name.startsWith(query)) return 0;
		if (name.includes(query)) return 1;
		return c.description.toLowerCase().includes(query) ? 2 : -1;
	};
	return commands
		.map((c) => ({ c, r: rank(c) }))
		.filter(({ r }) => r >= 0)
		.sort((a, b) => a.r - b.r || a.c.name.localeCompare(b.c.name))
		.map(({ c }) => c);
}
