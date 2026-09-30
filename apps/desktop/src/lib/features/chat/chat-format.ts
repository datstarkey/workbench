import type { TranscriptItem, TranscriptPatchHunk, TranscriptServerMsg } from '$types/workbench';

type ToolItem = Extract<TranscriptItem, { kind: 'tool' }>;

/** Tools that only look around. Runs of these collapse into one row of chips. */
const QUIET_TOOLS = new Set(['Read', 'Grep', 'Glob', 'LS', 'ToolSearch']);

export type ChatBlock =
	| { kind: 'item'; item: TranscriptItem }
	| { kind: 'quiet'; id: string; tools: ToolItem[] };

/** Apply a server frame: a snapshot replaces the list, an update upserts by id. */
export function mergeTranscript(
	items: TranscriptItem[],
	msg: Exclude<TranscriptServerMsg, { t: 'revoked' }>
): TranscriptItem[] {
	if (msg.t === 'snapshot') return msg.items;
	const next = items.slice();
	const at = new Map(next.map((item, i) => [item.id, i]));
	for (const item of msg.items) {
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

export type TextSegment = { kind: 'prose' | 'code'; text: string };

/**
 * Split Markdown code fences out of assistant text. Rendered as plain text —
 * never as HTML — because model output can quote untrusted repo content.
 */
export function splitFences(text: string): TextSegment[] {
	const segments: TextSegment[] = [];
	const parts = text.split(/^```[^\n]*\n?/m);
	parts.forEach((part, i) => {
		const body = i % 2 === 1 ? part.replace(/\n$/, '') : part.trim();
		if (body) segments.push({ kind: i % 2 === 1 ? 'code' : 'prose', text: body });
	});
	return segments;
}

export function formatTokens(n: number | null): string {
	if (n == null) return '';
	return n >= 1000 ? `${Math.round(n / 1000)}k` : String(n);
}
