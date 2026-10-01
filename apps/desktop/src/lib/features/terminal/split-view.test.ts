import { describe, expect, it } from 'vitest';
import { splitInset, visibleSplit } from './split-view';
import type { ProjectWorkspace, TerminalTabState } from '$types/workbench';

const tab = (id: string): TerminalTabState => ({
	id,
	label: id,
	split: 'horizontal',
	panes: [{ id: `${id}-pane` }]
});

function workspace(overrides: Partial<ProjectWorkspace> = {}): ProjectWorkspace {
	return {
		id: 'ws',
		projectPath: '/p',
		projectName: 'p',
		terminalTabs: [tab('a'), tab('b'), tab('c')],
		activeTerminalTabId: 'c',
		splitView: { direction: 'horizontal', tabIds: ['c', 'a'] },
		...overrides
	};
}

describe('visibleSplit', () => {
	it('returns the pair in tab-strip order while one of them is active', () => {
		const split = visibleSplit(workspace());

		expect(split?.direction).toBe('horizontal');
		expect(split?.tabs.map((t) => t.id)).toEqual(['a', 'c']);
	});

	it('hides the split while a tab outside it is active', () => {
		expect(visibleSplit(workspace({ activeTerminalTabId: 'b' }))).toBeNull();
	});

	it('ignores a split whose tab no longer exists', () => {
		expect(visibleSplit(workspace({ terminalTabs: [tab('b'), tab('c')] }))).toBeNull();
	});

	it('never splits native panes', () => {
		expect(visibleSplit(workspace({ renderer: 'native' }))).toBeNull();
	});
});

describe('splitInset', () => {
	it('places each half and leaves other tabs full size', () => {
		const horizontal = visibleSplit(workspace());
		expect(splitInset(horizontal, 'a')).toBe('0 50% 0 0');
		expect(splitInset(horizontal, 'c')).toBe('0 0 0 50%');
		expect(splitInset(horizontal, 'b')).toBe('0');

		const vertical = visibleSplit(
			workspace({ splitView: { direction: 'vertical', tabIds: ['a', 'c'] } })
		);
		expect(splitInset(vertical, 'a')).toBe('0 0 50% 0');
		expect(splitInset(vertical, 'c')).toBe('50% 0 0 0');
		expect(splitInset(null, 'a')).toBe('0');
	});
});
