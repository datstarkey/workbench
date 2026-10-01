import { describe, expect, it } from 'vitest';
import type { TranscriptItem, TranscriptMeta } from '$types/workbench';
import {
	activity,
	answerFor,
	applyChanges,
	approvalPreview,
	formatElapsed,
	formatTokens,
	groupBlocks,
	latestTodos,
	modeLabel,
	parseQuestions,
	patchStats,
	shortPath,
	splitFences,
	toolDetail
} from './chat-format';

const meta: TranscriptMeta = {
	title: null,
	model: null,
	permissionMode: null,
	contextTokens: null,
	busy: false
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

describe('splitFences', () => {
	it('separates prose from fenced code', () => {
		expect(splitFences('Run this:\n```bash\nbun test\n```\nThen check.')).toEqual([
			{ kind: 'prose', text: 'Run this:' },
			{ kind: 'code', text: 'bun test' },
			{ kind: 'prose', text: 'Then check.' }
		]);
	});

	it('returns plain prose unchanged', () => {
		expect(splitFences('Just words.')).toEqual([{ kind: 'prose', text: 'Just words.' }]);
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
