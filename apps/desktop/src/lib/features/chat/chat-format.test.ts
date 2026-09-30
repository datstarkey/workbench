import { describe, expect, it } from 'vitest';
import type { TranscriptItem, TranscriptMeta } from '$types/workbench';
import {
	formatTokens,
	groupBlocks,
	latestTodos,
	mergeTranscript,
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

describe('mergeTranscript', () => {
	it('replaces the list on a snapshot', () => {
		const next = mergeTranscript([text('a', 'old')], {
			t: 'snapshot',
			items: [text('b', 'new')],
			meta,
			truncated: false
		});
		expect(next.map((i) => i.id)).toEqual(['b']);
	});

	it('updates items in place by id and appends new ones', () => {
		const running = { ...tool('t1', 'Bash'), status: 'running' as const };
		const items = [text('a', 'hi'), running];
		const next = mergeTranscript(items, {
			t: 'update',
			items: [{ ...running, status: 'ok' }, text('c', 'done')],
			meta
		});
		expect(next.map((i) => i.id)).toEqual(['a', 't1', 'c']);
		expect(next[1]).toMatchObject({ status: 'ok' });
		expect(items[1]).toMatchObject({ status: 'running' });
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
