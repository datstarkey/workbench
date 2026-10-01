import type {
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

export type ChatBlock =
	| { kind: 'item'; item: TranscriptItem }
	| { kind: 'quiet'; id: string; tools: ToolItem[] };

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
	const blocks: ChatBlock[] = [];
	for (const item of items) {
		const last = blocks[blocks.length - 1];
		if (item.kind === 'tool' && QUIET_TOOLS.has(item.name) && item.status !== 'error') {
			if (last?.kind === 'quiet') last.tools.push(item);
			else blocks.push({ kind: 'quiet', id: item.id, tools: [item] });
		} else if (item.kind !== 'tool' || item.name !== 'TodoWrite') {
			blocks.push({ kind: 'item', item });
		}
	}
	return blocks;
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
}

/** The plan from the most recent TodoWrite call, if any. */
export function latestTodos(items: TranscriptItem[]): TodoStep[] {
	for (let i = items.length - 1; i >= 0; i--) {
		const item = items[i];
		if (item.kind !== 'tool' || item.name !== 'TodoWrite') continue;
		const todos = item.input?.todos;
		return Array.isArray(todos) ? (todos as TodoStep[]) : [];
	}
	return [];
}

export function formatTokens(n: number | null): string {
	if (n == null) return '';
	return n >= 1000 ? `${Math.round(n / 1000)}k` : String(n);
}

export interface QuestionOption {
	label: string;
	description: string;
	preview?: string;
}

export interface Question {
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

/** What Claude is doing right now, for the activity line under the chat. */
export type Activity =
	| { kind: 'idle' }
	| { kind: 'retrying'; retry: RetryInfo }
	| { kind: 'approval'; approval: ApprovalItem }
	| { kind: 'tool'; tool: ToolItem }
	| { kind: 'writing' }
	| { kind: 'thinking' };

export function activity(items: TranscriptItem[], meta: TranscriptMeta | null): Activity {
	for (let i = items.length - 1; i >= 0; i--) {
		const item = items[i];
		if (item.kind === 'approval' && !item.decision && !item.expired) {
			return { kind: 'approval', approval: item };
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
	mode: PermissionMode;
	label: string;
	hint: string;
}

/** Modes offered in the picker, in Shift+Tab order. */
export const MODE_OPTIONS: ModeOption[] = [
	{ mode: 'default', label: 'Ask first', hint: 'Claude asks before edits and commands' },
	{ mode: 'acceptEdits', label: 'Accept edits', hint: 'File edits run; commands still ask' },
	{ mode: 'plan', label: 'Plan', hint: 'Claude reads and plans, changes nothing' },
	{ mode: 'auto', label: 'Auto', hint: 'A safety check approves routine actions' },
	{ mode: 'bypassPermissions', label: 'Bypass', hint: 'Nothing asks first' }
];

export function modeLabel(mode: PermissionMode | null | undefined): string {
	if (mode === 'dontAsk') return "Don't ask";
	return MODE_OPTIONS.find((m) => m.mode === mode)?.label ?? 'Ask first';
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

/** What to tell the person about their usage limit, if anything. */
export function limitNotice(
	info: RateLimitInfo | null,
	formatTime: (unixSeconds: number) => string
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

/** `38 KB`, `1.2 MB`. */
export function formatBytes(n: number): string {
	return n >= 1024 * 1024 ? `${(n / 1024 / 1024).toFixed(1)} MB` : `${Math.ceil(n / 1024)} KB`;
}

/** How full the context window is, 0–1; `[1m]` on the model means a 1M window, else 200k. */
export function contextUsed(meta: TranscriptMeta | null): number {
	if (!meta?.contextTokens) return 0;
	const limit = meta.model?.endsWith('[1m]') ? 1_000_000 : 200_000;
	return Math.min(1, meta.contextTokens / limit);
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

const EFFORT_LABELS: Record<EffortLevel, string> = {
	low: 'Low',
	medium: 'Medium',
	high: 'High',
	xhigh: 'Extra high',
	max: 'Max'
};

export function effortLabel(level: EffortLevel | null): string {
	return level ? EFFORT_LABELS[level] : 'Default effort';
}

/** The `/` command being typed, if the draft is just `/` plus a name so far. */
export function slashQuery(draft: string): string | null {
	const match = /^\/(\S*)$/.exec(draft);
	return match ? match[1].toLowerCase() : null;
}

/** Commands for the `/` menu: name prefix matches, then name, then description matches. */
export function matchCommands(commands: SlashCommand[], query: string): SlashCommand[] {
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
