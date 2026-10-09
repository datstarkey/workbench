import { describe, expect, it } from 'vitest';
import type { TaskInfo, TranscriptItem, TranscriptMeta } from '@workbench/types';
import type { ToolItem } from './chat-format';
import {
	activity,
	agentName,
	answerFor,
	applyChanges,
	contextUsed,
	canPickModel,
	currentModel,
	effortLabel,
	formatBytes,
	formatCount,
	limitNotice,
	chatTitle,
	matchCommands,
	slashQuery,
	isWholeCommand,
	insertCommand,
	pickTasks,
	pickTaskView,
	taskViews,
	approvalPreview,
	formatElapsed,
	groupBlocks,
	jsonAsMarkdown,
	outputMarkdown,
	stepNames,
	latestTodos,
	modeLabel,
	modeOptions,
	isRiskyMode,
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

describe('contextUsed', () => {
	it('measures against a 1M window only when the model says [1m]', () => {
		const used = { ...meta, contextTokens: 126_000 };
		expect(contextUsed({ ...used, model: 'claude-opus-5-5[1m]' })).toBeCloseTo(0.126);
		expect(contextUsed({ ...used, model: 'claude-opus-5-5' })).toBeCloseTo(0.63);
		expect(contextUsed(meta)).toBe(0);
	});

	it('uses the window the session reports (Codex)', () => {
		const used = { ...meta, contextTokens: 129_200, contextWindow: 258_400 };
		expect(contextUsed({ ...used, model: 'gpt-6.1-sol' })).toBeCloseTo(0.5);
	});
});

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

	it('offers Codex its own presets, and says when its config is in charge', () => {
		expect(modeOptions('codex').map((m) => m.mode)).toEqual(['read-only', 'auto', 'full-access']);
		expect(modeOptions().map((m) => m.mode)).toContain('acceptEdits');
		expect(modeLabel('full-access', 'codex')).toBe('Full access');
		expect(modeLabel(null, 'codex')).toBe('Codex config');
		expect(isRiskyMode('full-access')).toBe(true);
		expect(isRiskyMode('bypassPermissions')).toBe(true);
		expect(isRiskyMode('auto')).toBe(false);
		expect(agentName('codex')).toBe('Codex');
		expect(agentName()).toBe('Claude');
	});

	it('formats the turn timer', () => {
		expect(formatElapsed(0)).toBe('0:00');
		expect(formatElapsed(65_400)).toBe('1:05');
	});
});

describe('tool output', () => {
	it('fences text that is only JSON, exactly as written', () => {
		expect(jsonAsMarkdown(' {"id": 12345678901234567890, "2": 1.0} ')).toBe(
			'```json\n{"id": 12345678901234567890, "2": 1.0}\n```'
		);
		expect(jsonAsMarkdown('{"code":"```"}')).toBe('````json\n{"code":"```"}\n````');
		expect(jsonAsMarkdown('{"cut": "abc…')).toBe('{"cut": "abc…');
		expect(jsonAsMarkdown('[Image] ok')).toBe('[Image] ok');
	});

	it('renders JSON, skills and subagents as Markdown, the rest as text', () => {
		expect(outputMarkdown(tool('s', 'Skill'), '# Help')).toBe('# Help');
		expect(outputMarkdown(tool('a', 'Agent'), '[1]')).toBe('```json\n[1]\n```');
		expect(outputMarkdown(tool('m', 'mcp__x__get'), '[]')).toBe('```json\n[]\n```');
		expect(outputMarkdown(tool('b', 'Bash'), '# not a heading')).toBe(null);
	});
});

describe('groupBlocks', () => {
	it('collapses runs of read-only tools and hides TodoWrite', () => {
		const running = { ...tool('b1', 'Bash'), status: 'running' as const };
		const failedPlan = { ...tool('tu', 'TaskUpdate'), status: 'error' as const };
		const blocks = groupBlocks([
			tool('r1', 'Read'),
			tool('todo', 'TodoWrite'),
			tool('tc', 'TaskCreate'),
			failedPlan,
			running,
			tool('g1', 'Grep'),
			text('a', 'Found it.')
		]);
		expect(blocks).toEqual([
			{ kind: 'quiet', id: 'r1', tools: [tool('r1', 'Read')] },
			{ kind: 'item', item: failedPlan },
			{ kind: 'item', item: running },
			{ kind: 'quiet', id: 'g1', tools: [tool('g1', 'Grep')] },
			{ kind: 'item', item: text('a', 'Found it.') }
		]);
	});

	it('folds finished calls and the thinking between them into one row', () => {
		const thinking: TranscriptItem = { kind: 'thinking', id: 'th', text: 'hmm' };
		const failed = { ...tool('b2', 'Bash'), status: 'error' as const };
		const blocks = groupBlocks([
			text('a', 'Looking around.'),
			tool('r1', 'Read'),
			thinking,
			tool('b1', 'Bash'),
			tool('e1', 'Edit'),
			failed,
			tool('b3', 'Bash')
		]);
		expect(blocks.map((b) => b.kind)).toEqual(['item', 'steps', 'item', 'item']);
		const steps = blocks[1];
		if (steps.kind !== 'steps') throw new Error('expected steps');
		expect(steps.id).toBe('r1');
		expect(steps.tools.map((t) => t.id)).toEqual(['r1', 'b1', 'e1']);
		expect(stepNames(steps.tools)).toBe('Read, Bash, Edit');
	});

	it('never folds an artifact card away', () => {
		const blocks = groupBlocks([tool('b1', 'Bash'), tool('a1', 'Artifact'), tool('b2', 'Bash')]);
		expect(blocks.map((b) => b.kind)).toEqual(['item', 'item', 'item']);
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

	const result = (item: ToolItem, output: string): ToolItem => ({ ...item, output });
	const create = (id: string, subject: string, output?: string): ToolItem => {
		const item = tool(id, 'TaskCreate', { subject, description: 'd', activeForm: `${subject}ing` });
		return output === undefined ? { ...item, status: 'running' } : result(item, output);
	};

	it('builds the plan from TaskCreate and TaskUpdate', () => {
		const items: TranscriptItem[] = [
			create('c1', 'Read', 'Task #1 created successfully: Read'),
			create('c2', 'Write', 'Task #2 created successfully: Write'),
			create('c3', 'Ship', 'Task #3 created successfully: Ship'),
			tool('u1', 'TaskUpdate', { taskId: '1', status: 'completed' }),
			tool('u2', 'TaskUpdate', { taskId: '2', status: 'in_progress', subject: 'Write it' }),
			tool('u3', 'TaskUpdate', { taskId: '3', status: 'deleted' }),
			tool('u4', 'TaskUpdate', { taskId: '9', status: 'completed' }),
			{ ...tool('u5', 'TaskUpdate', { taskId: '1', status: 'pending' }), status: 'error' }
		];
		expect(latestTodos(items)).toEqual([
			{ content: 'Read', status: 'completed', activeForm: 'Reading' },
			{ content: 'Write it', status: 'in_progress', activeForm: 'Writeing' }
		]);
	});

	it('counts a TaskCreate once it has a result, by the id in it', () => {
		const items: TranscriptItem[] = [
			create('c1', 'A', 'Task #7 created successfully: A'),
			create('c2', 'B', 'some other wording'),
			create('c3', 'Pending'),
			{ ...tool('c4', 'TaskCreate'), input: null, status: 'running' },
			tool('u1', 'TaskUpdate', { taskId: '8', status: 'completed' })
		];
		expect(latestTodos(items).map((s) => [s.content, s.status])).toEqual([
			['A', 'pending'],
			['B', 'completed']
		]);
	});

	it('keeps the plan while a TodoWrite streams', () => {
		const plan = tool('t1', 'TodoWrite', { todos: [{ content: 'a', status: 'pending' }] });
		const streaming: TranscriptItem = { ...plan, id: 't2', input: null, status: 'running' };
		expect(latestTodos([plan, streaming])).toEqual([{ content: 'a', status: 'pending' }]);
	});

	it('replaces the plan with a TaskList result or a newer TodoWrite', () => {
		const list = result(
			tool('l1', 'TaskList'),
			'#1 [completed] Read (agent)\n#2 [in_progress] Renamed [blocked by #1]\n#5 [pending] Test (x) [blocked by #2]'
		);
		const items: TranscriptItem[] = [
			create('c1', 'Read', 'Task #1 created successfully: Read'),
			create('c2', 'Old', 'Task #2 created successfully: Old'),
			create('c3', 'Gone', 'Task #3 created successfully: Gone'),
			list,
			create('c4', 'Next', 'oops')
		];
		expect(latestTodos(items)).toEqual([
			{ content: 'Read', status: 'completed', activeForm: 'Reading' },
			{ content: 'Renamed', status: 'in_progress', activeForm: 'Olding' },
			{ content: 'Test (x)', status: 'pending', activeForm: undefined },
			{ content: 'Next', status: 'pending', activeForm: 'Nexting' }
		]);
		expect(latestTodos([...items, result(tool('l2', 'TaskList'), 'No tasks found')])).toEqual([]);
		expect(
			latestTodos([
				...items,
				tool('t', 'TodoWrite', { todos: [{ content: 'only', status: 'pending' }] })
			])
		).toEqual([{ content: 'only', status: 'pending' }]);
		expect(latestTodos([...items.slice(0, 3), { ...list, output: undefined }])).toHaveLength(3);
	});
});

describe('chatTitle', () => {
	const user = (text: string) => ({ kind: 'user', id: text || 'empty', text }) as TranscriptItem;
	it('prefers the title, then the first prompt, then the agent', () => {
		const items = [user(''), user('Fix the build'), user('And the tests')];
		expect(chatTitle(items, { title: 'Build fix' } as TranscriptMeta, 'claude')).toBe('Build fix');
		expect(chatTitle(items, null, 'claude')).toBe('Fix the build');
		expect(chatTitle([], null, 'codex')).toBe('Codex');
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

	it('splits agents from other jobs in start order', () => {
		const tasks = [
			task('a-done', 'agent', 'completed'),
			task('shell', 'local_bash', 'running'),
			task('a-live', 'agent', 'running')
		];
		const { agents, jobs, tab, selected } = pickTasks(tasks, null, null);
		expect(agents.map((t) => t.id)).toEqual(['a-done', 'a-live']);
		expect(jobs.map((t) => t.id)).toEqual(['shell']);
		expect(tab).toBe('agents');
		expect(selected?.id).toBe('a-done');
	});

	it('keeps the pick while it exists and falls back when it is gone', () => {
		const tasks = [task('shell', 'local_bash', 'running'), task('a1', 'agent', 'running')];
		expect(pickTasks(tasks, 'jobs', null).selected?.id).toBe('shell');
		expect(pickTasks(tasks, 'agents', 'a1').selected?.id).toBe('a1');
		expect(pickTasks(tasks, 'agents', 'gone').selected?.id).toBe('a1');
		const onlyJobs = [task('shell', 'local_bash', 'running')];
		expect(pickTasks(onlyJobs, 'agents', null).tab).toBe('jobs');
		expect(pickTasks([], null, null).selected).toBeNull();
	});

	it('opens an agent on its conversation and a shell on its output', () => {
		const agent = task('a1', 'agent', 'running');
		const shell = task('shell', 'local_bash', 'running');
		expect(taskViews(agent)).toEqual(['conversation', 'output']);
		expect(pickTaskView(agent, null)).toBe('conversation');
		expect(pickTaskView(agent, 'output')).toBe('output');
		expect(taskViews(shell)).toEqual(['output']);
		expect(pickTaskView(shell, 'conversation')).toBe('output');
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

	it('offers only models with a resolved id, besides the current one', () => {
		const guessed = { ...models[2], value: 'haiku', resolvedModel: null };
		expect(canPickModel(models[2], null)).toBe(true);
		expect(canPickModel(guessed, models[0])).toBe(false);
		expect(canPickModel(guessed, guessed)).toBe(true);
	});

	it('labels effort', () => {
		expect(effortLabel('xhigh')).toBe('Extra high');
		expect(effortLabel('none')).toBe('No reasoning');
		expect(effortLabel('minimal')).toBe('Minimal');
		expect(effortLabel(null)).toBe('Default effort');
	});
});

describe('slash menu', () => {
	const commands = [
		{ name: 'compact', description: 'Free up context' },
		{ name: 'clear', description: 'Start a new conversation' },
		{ name: 'review', description: 'Review a pull request' },
		{ name: 'security-review', description: 'Find vulnerabilities' }
	];

	it('opens for a command name at the caret, at the start or mid-line', () => {
		expect(slashQuery('/')).toEqual({ start: 0, query: '' });
		expect(slashQuery('/Com')).toEqual({ start: 0, query: 'com' });
		expect(slashQuery('/compact focus on tests')).toBe(null);
		expect(slashQuery('fix it then /rev')).toEqual({ start: 12, query: 'rev' });
		expect(slashQuery('fix it then /rev and more', 16)).toEqual({ start: 12, query: 'rev' });
		expect(slashQuery('see src/lib')).toBe(null);
		expect(slashQuery('open /usr/bin')).toBe(null);
	});

	it('treats a command as the whole draft only with nothing but whitespace around it', () => {
		const at = (draft: string, caret = draft.length) => slashQuery(draft, caret)!;
		expect(isWholeCommand('/res', at('/res'), 4)).toBe(true);
		expect(isWholeCommand('/resume', at('/resume', 4), 4)).toBe(true);
		expect(isWholeCommand('  /res', at('  /res'), 6)).toBe(true);
		expect(isWholeCommand('fix it /rev', at('fix it /rev'), 11)).toBe(false);
		expect(isWholeCommand('/rev then more', at('/rev then more', 4), 4)).toBe(false);
	});

	it('inserts a picked command where it was typed', () => {
		const draft = 'fix it then /re and push';
		expect(insertCommand(draft, slashQuery(draft, 15)!, 15, 'review')).toEqual({
			text: 'fix it then /review and push',
			caret: 20
		});
	});

	it('ranks prefix matches before other matches', () => {
		expect(matchCommands(commands, 'rev').map((c) => c.name)).toEqual([
			'review',
			'security-review'
		]);
		expect(matchCommands(commands, 'context').map((c) => c.name)).toEqual(['compact']);
		expect(matchCommands(commands, '')).toHaveLength(4);
	});

	it("matches a plugin command by its own name and its name's words", () => {
		const all = [
			...commands,
			{ name: 'starkeydigital:app-signing', description: 'Sign the app' },
			{ name: 'add-dir', description: 'Add a working directory' },
			{ name: 'agents', description: 'Manage agents' }
		];
		const names = (query: string) => matchCommands(all, query).map((c) => c.name);
		expect(names('a').slice(0, 3)).toEqual(['add-dir', 'agents', 'starkeydigital:app-signing']);
		expect(names('app')[0]).toBe('starkeydigital:app-signing');
		expect(names('sign')[0]).toBe('starkeydigital:app-signing');
		expect(names('starkeydigital:a')).toEqual(['starkeydigital:app-signing']);
		expect(names('view')).toEqual(['review', 'security-review']);
	});

	it('lists a command name once', () => {
		const twice = [...commands, { name: 'review', description: 'A plugin’s' }];
		for (const query of ['', 'rev']) {
			const found = matchCommands(twice, query).filter((c) => c.name === 'review');
			expect(found).toEqual([commands.find((c) => c.name === 'review')]);
		}
	});
});
