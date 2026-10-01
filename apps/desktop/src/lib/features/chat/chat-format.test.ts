import { describe, expect, it } from 'vitest';
import type { TaskInfo, TranscriptItem, TranscriptMeta } from '$types/workbench';
import {
	activity,
	answerFor,
	applyChanges,
	currentModel,
	effortLabel,
	formatBytes,
	formatCount,
	limitNotice,
	sortTasks,
	approvalPreview,
	formatElapsed,
	formatTokens,
	groupBlocks,
	latestTodos,
	modeLabel,
	parseQuestions,
	patchStats,
	shortPath,
	toolDetail
} from './chat-format';

const meta: TranscriptMeta = {
	title: null,
	model: null,
	permissionMode: null,
	contextTokens: null,
	busy: false,
	tasks: [],
	retry: null,
	rateLimit: null,
	models: [],
	modelChoice: null,
	effort: null
};

function tool(id: string, name: string, input: Record<string, unknown> = {}) {
	return { kind: 'tool', id, name, input, status: 'ok' } as const satisfies TranscriptItem;
}

const text = (id: string, t: string): TranscriptItem => ({ kind: 'text', id, text: t });

describe('applyChanges', () => {
	it('updates items in place by id and appends new ones', () => {
		const running = { ...tool('t1', 'Bash'), status: 'running' as const };
		const items = [text('a', 'hi'), running];
		const next = applyChanges(items, 0, [
			[1, { ...running, status: 'ok' }],
			[2, text('c', 'done')]
		]);
		expect(next.map((i) => i.id)).toEqual(['a', 't1', 'c']);
		expect(next[1]).toMatchObject({ status: 'ok' });
		expect(items[1]).toMatchObject({ status: 'running' });
	});

	it('ignores changes to history the snapshot left out', () => {
		const next = applyChanges([text('b', 'kept')], 10, [[3, text('old', 'from history')]]);
		expect(next.map((i) => i.id)).toEqual(['b']);
	});
});

describe('activity', () => {
	const busy = { ...meta, busy: true };
	const approval = (id: string, extra = {}): TranscriptItem => ({
		kind: 'approval',
		id,
		tool: 'Bash',
		input: { command: 'ls' },
		canAlwaysAllow: false,
		expired: false,
		...extra
	});

	it('puts an open approval first, even mid-turn', () => {
		expect(activity([approval('r1')], busy)).toMatchObject({ kind: 'approval' });
		expect(activity([approval('r1', { decision: 'allow' })], meta)).toEqual({ kind: 'idle' });
		expect(activity([approval('r1', { expired: true })], meta)).toEqual({ kind: 'idle' });
	});

	it('names what a busy turn is doing', () => {
		const running = { ...tool('t', 'Bash'), status: 'running' as const };
		expect(activity([running], busy)).toMatchObject({ kind: 'tool' });
		expect(activity([text('a', 'Writing now')], busy)).toEqual({ kind: 'writing' });
		expect(activity([text('a', '')], busy)).toEqual({ kind: 'thinking' });
		expect(activity([], busy)).toEqual({ kind: 'thinking' });
	});
});

describe('approvalPreview', () => {
	it('shows the command, else the file, else the input', () => {
		const base = { kind: 'approval', id: 'r', canAlwaysAllow: false, expired: false } as const;
		expect(approvalPreview({ ...base, tool: 'Bash', input: { command: 'git push' } })).toBe(
			'git push'
		);
		expect(
			approvalPreview({ ...base, tool: 'Edit', input: { file_path: '/repo/a.ts' } }, '/repo')
		).toBe('a.ts');
		expect(approvalPreview({ ...base, tool: 'mcp__x', input: { n: 1 } })).toContain('"n": 1');
	});
});

describe('modes and time', () => {
	it('labels permission modes', () => {
		expect(modeLabel('acceptEdits')).toBe('Accept edits');
		expect(modeLabel(null)).toBe('Ask first');
		expect(modeLabel('dontAsk')).toBe("Don't ask");
	});

	it('formats the turn timer', () => {
		expect(formatElapsed(0)).toBe('0:00');
		expect(formatElapsed(65_400)).toBe('1:05');
	});
});

describe('groupBlocks', () => {
	it('collapses runs of read-only tools and hides TodoWrite', () => {
		const blocks = groupBlocks([
			text('a', 'Looking around.'),
			tool('r1', 'Read'),
			tool('g1', 'Grep'),
			tool('todo', 'TodoWrite'),
			tool('e1', 'Edit'),
			tool('r2', 'Read')
		]);
		expect(blocks.map((b) => (b.kind === 'quiet' ? b.tools.map((t) => t.id) : b.item.id))).toEqual([
			'a',
			['r1', 'g1'],
			'e1',
			['r2']
		]);
	});

	it('keeps a failed read as its own card', () => {
		const failed = { ...tool('r1', 'Read'), status: 'error' as const };
		expect(groupBlocks([failed])).toEqual([{ kind: 'item', item: failed }]);
	});
});

describe('toolDetail', () => {
	it('shows the first line of a shell command', () => {
		expect(toolDetail(tool('b', 'Bash', { command: 'bun test\n# more' }))).toBe('bun test');
	});

	it('shows file paths relative to the session cwd', () => {
		expect(toolDetail(tool('e', 'Edit', { file_path: '/repo/src/a.ts' }), '/repo')).toBe(
			'src/a.ts'
		);
	});

	it('falls back to the first string input for unknown tools', () => {
		expect(toolDetail(tool('m', 'mcp__x__y', { n: 1, q: 'find me' }))).toBe('find me');
	});
});

describe('shortPath', () => {
	it('handles Windows separators', () => {
		expect(shortPath('C:\\repo\\src\\a.ts', 'C:\\repo')).toBe('src\\a.ts');
	});

	it('does not strip a sibling directory that shares a prefix', () => {
		expect(shortPath('/repo-other/a.ts', '/repo')).toBe('/repo-other/a.ts');
	});
});

describe('patchStats', () => {
	it('counts added and removed lines across hunks', () => {
		expect(
			patchStats([
				{ oldStart: 1, newStart: 1, lines: [' a', '-b', '+c', '+d'] },
				{ oldStart: 9, newStart: 10, lines: ['-e'] }
			])
		).toEqual({ added: 2, removed: 2 });
	});
});

describe('latestTodos', () => {
	it('returns the plan from the most recent TodoWrite', () => {
		const items = [
			tool('t1', 'TodoWrite', { todos: [{ content: 'old', status: 'pending' }] }),
			tool('t2', 'TodoWrite', { todos: [{ content: 'new', status: 'completed' }] })
		];
		expect(latestTodos(items)).toEqual([{ content: 'new', status: 'completed' }]);
		expect(latestTodos([text('a', 'x')])).toEqual([]);
	});
});

describe('formatTokens', () => {
	it('abbreviates thousands', () => {
		expect(formatTokens(225684)).toBe('226k');
		expect(formatTokens(512)).toBe('512');
		expect(formatTokens(null)).toBe('');
	});
});

describe('questions', () => {
	it('parses AskUserQuestion input and drops malformed entries', () => {
		const qs = parseQuestions({
			questions: [
				{
					question: 'Which library?',
					header: 'Library',
					multiSelect: false,
					options: [
						{ label: 'date-fns', description: 'Small', preview: 'import { format }' },
						{ label: 'dayjs' },
						{ nope: true }
					]
				},
				{ header: 'no question text' }
			]
		});
		expect(qs).toEqual([
			{
				question: 'Which library?',
				header: 'Library',
				multiSelect: false,
				options: [
					{ label: 'date-fns', description: 'Small', preview: 'import { format }' },
					{ label: 'dayjs', description: '' }
				]
			}
		]);
		expect(parseQuestions(null)).toEqual([]);
	});

	it('joins picks and own words the way the tool reads them', () => {
		expect(answerFor(['A'], '')).toBe('A');
		expect(answerFor(['A', 'B'], ' also C ')).toBe('A, B, also C');
		expect(answerFor([], '')).toBe('');
	});
});

describe('tasks panel', () => {
	const task = (id: string, kind: string, status: TaskInfo['status']): TaskInfo => ({
		id,
		kind,
		status,
		description: id,
		background: false,
		toolUses: 0,
		tokens: 0,
		durationMs: 0
	});

	it('splits agents from other jobs, running first', () => {
		const { agents, jobs } = sortTasks([
			task('a-done', 'agent', 'completed'),
			task('shell', 'local_bash', 'running'),
			task('a-live', 'agent', 'running')
		]);
		expect(agents.map((t) => t.id)).toEqual(['a-live', 'a-done']);
		expect(jobs.map((t) => t.id)).toEqual(['shell']);
	});

	it('abbreviates counts', () => {
		expect(formatCount(7, 'tool calls')).toBe('7 tool calls');
		expect(formatCount(1800, 'tokens')).toBe('1.8k tokens');
		expect(formatCount(24057, 'tokens')).toBe('24k tokens');
	});
});

describe('limits and retries', () => {
	const at = () => '15:00';
	it('says when the limit blocks and when it resets', () => {
		expect(
			limitNotice({ status: 'rejected', resetsAt: 1, kind: 'five_hour', utilization: 1 }, at)
		).toEqual({
			tone: 'blocked',
			text: "You've reached your 5-hour usage limit. It resets at 15:00."
		});
		expect(
			limitNotice(
				{ status: 'allowed_warning', resetsAt: null, kind: 'seven_day', utilization: 0.85 },
				at
			)
		).toEqual({ tone: 'warn', text: "You've used 85% of your weekly usage limit." });
		expect(
			limitNotice({ status: 'allowed', resetsAt: null, kind: null, utilization: 0.1 }, at)
		).toBe(null);
	});

	it('shows a retry ahead of other activity', () => {
		const retry = { attempt: 2, maxRetries: 10, retryDelayMs: 4000, error: 'overloaded' };
		expect(activity([], { ...meta, busy: true, retry })).toEqual({ kind: 'retrying', retry });
	});

	it('formats sizes', () => {
		expect(formatBytes(38_000)).toBe('38 KB');
		expect(formatBytes(1_300_000)).toBe('1.2 MB');
	});
});

describe('model and effort', () => {
	const models = [
		{
			value: 'default',
			displayName: 'Default',
			description: '',
			resolvedModel: 'claude-opus-5-5',
			effortLevels: []
		},
		{
			value: 'opus',
			displayName: 'Opus 5.5',
			description: '',
			resolvedModel: 'claude-opus-5-5',
			effortLevels: []
		},
		{
			value: 'sonnet',
			displayName: 'Sonnet 5.5',
			description: '',
			resolvedModel: 'claude-sonnet-5-5',
			effortLevels: []
		}
	];

	it('shows the picked model, else the running one, else the default', () => {
		expect(currentModel({ ...meta, models, modelChoice: 'sonnet' })?.value).toBe('sonnet');
		expect(currentModel({ ...meta, models, model: 'claude-opus-5-5[1m]' })?.value).toBe('opus');
		expect(currentModel({ ...meta, models, model: 'something-else' })?.value).toBe('default');
		expect(currentModel({ ...meta, models: [] })).toBe(null);
	});

	it('labels effort', () => {
		expect(effortLabel('xhigh')).toBe('Extra high');
		expect(effortLabel(null)).toBe('Default effort');
	});
});
