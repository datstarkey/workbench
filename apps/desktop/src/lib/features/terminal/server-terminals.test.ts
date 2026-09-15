// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import type { ProjectWorkspace } from '$types/workbench';
import {
	adoptableTerminals,
	adoptionWorkspace,
	paneDisplayName,
	withoutPanes,
	TerminalAdoptionPoller,
	type AdoptableTerminal,
	type AdoptionPollerDeps
} from './server-terminals';

const term = (id: string, overrides: Partial<AdoptableTerminal> = {}): AdoptableTerminal => ({
	id,
	name: `t-${id}`,
	cwd: '/p',
	alive: true,
	...overrides
});

const ws = (overrides: Partial<ProjectWorkspace>): ProjectWorkspace => ({
	id: 'ws',
	projectPath: '/p',
	projectName: 'app',
	terminalTabs: [],
	activeTerminalTabId: '',
	...overrides
});

describe('adoptableTerminals', () => {
	it('keeps only live terminals that are neither mapped nor claimed locally', () => {
		const list = [term('mapped'), term('claimed'), term('dead', { alive: false }), term('new')];
		const result = adoptableTerminals(list, new Set(['mapped']), (id) => id === 'claimed');
		expect(result.map((t) => t.id)).toEqual(['new']);
	});
});

describe('adoptionWorkspace', () => {
	const main = ws({ id: 'main' });
	const worktree = ws({ id: 'wt', worktreePath: '/p-feat', branch: 'feat' });

	it('prefers the worktree workspace for a worktree cwd', () => {
		expect(adoptionWorkspace([main, worktree], '/p-feat')?.id).toBe('wt');
	});

	it('falls back to the main workspace for the project path', () => {
		expect(adoptionWorkspace([worktree, main], '/p')?.id).toBe('main');
	});

	it('never picks a native-renderer workspace or an unrelated one', () => {
		expect(adoptionWorkspace([ws({ renderer: 'native' })], '/p')).toBeUndefined();
		expect(adoptionWorkspace([main], '/elsewhere')).toBeUndefined();
	});
});

describe('withoutPanes', () => {
	const tabs = [
		{ id: 't1', label: 'mine', split: 'horizontal' as const, panes: [{ id: 'a' }] },
		{ id: 't2', label: 'remote', split: 'horizontal' as const, panes: [{ id: 'b' }] },
		{ id: 't3', label: 'mixed', split: 'vertical' as const, panes: [{ id: 'c' }, { id: 'd' }] }
	];

	it('drops the panes, empty tabs, and repoints a dropped active tab', () => {
		const [result] = withoutPanes(
			[ws({ terminalTabs: tabs, activeTerminalTabId: 't2' })],
			new Set(['b', 'd'])
		);
		expect(result.terminalTabs.map((t) => [t.id, t.panes.map((p) => p.id)])).toEqual([
			['t1', ['a']],
			['t3', ['c']]
		]);
		expect(result.activeTerminalTabId).toBe('t1');
	});

	it('returns the workspaces untouched when there is nothing to drop', () => {
		const workspaces = [ws({ terminalTabs: tabs })];
		expect(withoutPanes(workspaces, new Set())).toBe(workspaces);
	});
});

describe('paneDisplayName', () => {
	it('names a pane by project, branch, tab and split index', () => {
		const workspaces = [
			ws({
				branch: 'feat',
				terminalTabs: [
					{ id: 't1', label: 'Claude 1', split: 'horizontal', panes: [{ id: 'a' }] },
					{ id: 't2', label: 'Shell', split: 'vertical', panes: [{ id: 'b' }, { id: 'c' }] }
				]
			})
		];
		expect(paneDisplayName(workspaces, 'a')).toBe('app [feat] · Claude 1');
		expect(paneDisplayName(workspaces, 'c')).toBe('app [feat] · Shell (2)');
		expect(paneDisplayName(workspaces, 'missing')).toBeUndefined();
	});
});

describe('TerminalAdoptionPoller', () => {
	let listTerminals: Mock<AdoptionPollerDeps['listTerminals']>;
	let adopt: Mock<AdoptionPollerDeps['adopt']>;
	let onAdopted: Mock<(terminal: AdoptableTerminal) => void>;
	let deps: AdoptionPollerDeps;
	let poller: TerminalAdoptionPoller;

	beforeEach(() => {
		vi.useFakeTimers();
		listTerminals = vi.fn(async () => [term('known'), term('foreign')]);
		adopt = vi.fn(() => true);
		onAdopted = vi.fn();
		deps = {
			listTerminals,
			isClaimed: () => false,
			knownIds: () => ['known'],
			adopt,
			onAdopted,
			intervalMs: 1000
		};
		poller = new TerminalAdoptionPoller(deps);
	});

	afterEach(() => {
		poller.dispose();
		vi.useRealTimers();
		vi.restoreAllMocks();
	});

	it('adopts unknown terminals and reports each adoption', async () => {
		await poller.tick();
		expect(adopt).toHaveBeenCalledTimes(1);
		expect(adopt).toHaveBeenCalledWith(expect.objectContaining({ id: 'foreign' }));
		expect(onAdopted).toHaveBeenCalledWith(expect.objectContaining({ id: 'foreign' }));
	});

	it('does not report a terminal no workspace could host', async () => {
		adopt.mockReturnValue(false);
		await poller.tick();
		expect(onAdopted).not.toHaveBeenCalled();
	});

	it('skips a round when the list is unavailable', async () => {
		listTerminals.mockResolvedValue(null);
		await poller.tick();
		expect(adopt).not.toHaveBeenCalled();
	});

	it('does not poll while the window is hidden', async () => {
		vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('hidden');
		await poller.tick();
		expect(listTerminals).not.toHaveBeenCalled();
	});

	it('polls on start, on the interval and on window focus until disposed', async () => {
		poller.start();
		await vi.advanceTimersByTimeAsync(0);
		expect(listTerminals).toHaveBeenCalledTimes(1);

		await vi.advanceTimersByTimeAsync(1000);
		expect(listTerminals).toHaveBeenCalledTimes(2);

		window.dispatchEvent(new Event('focus'));
		await vi.advanceTimersByTimeAsync(0);
		expect(listTerminals).toHaveBeenCalledTimes(3);

		poller.dispose();
		window.dispatchEvent(new Event('focus'));
		await vi.advanceTimersByTimeAsync(5000);
		expect(listTerminals).toHaveBeenCalledTimes(3);
	});
});
