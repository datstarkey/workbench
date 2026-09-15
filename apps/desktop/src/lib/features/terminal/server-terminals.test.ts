// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ProjectWorkspace } from '$types/workbench';
import {
	adoptableTerminals,
	adoptionWorkspace,
	paneDisplayName,
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
	let deps: AdoptionPollerDeps & {
		listTerminals: ReturnType<typeof vi.fn>;
		adopt: ReturnType<typeof vi.fn>;
		onAdopted: ReturnType<typeof vi.fn>;
	};
	let poller: TerminalAdoptionPoller;

	beforeEach(() => {
		vi.useFakeTimers();
		deps = {
			listTerminals: vi.fn(async () => [term('known'), term('foreign')]),
			isClaimed: () => false,
			knownIds: () => ['known'],
			adopt: vi.fn(() => true),
			onAdopted: vi.fn(),
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
		expect(deps.adopt).toHaveBeenCalledTimes(1);
		expect(deps.adopt).toHaveBeenCalledWith(expect.objectContaining({ id: 'foreign' }));
		expect(deps.onAdopted).toHaveBeenCalledWith(expect.objectContaining({ id: 'foreign' }));
	});

	it('does not report a terminal no workspace could host', async () => {
		deps.adopt.mockReturnValue(false);
		await poller.tick();
		expect(deps.onAdopted).not.toHaveBeenCalled();
	});

	it('skips a round when the list is unavailable', async () => {
		deps.listTerminals.mockResolvedValue(null);
		await poller.tick();
		expect(deps.adopt).not.toHaveBeenCalled();
	});

	it('does not poll while the window is hidden', async () => {
		vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('hidden');
		await poller.tick();
		expect(deps.listTerminals).not.toHaveBeenCalled();
	});

	it('polls on start, on the interval and on window focus until disposed', async () => {
		poller.start();
		await vi.advanceTimersByTimeAsync(0);
		expect(deps.listTerminals).toHaveBeenCalledTimes(1);

		await vi.advanceTimersByTimeAsync(1000);
		expect(deps.listTerminals).toHaveBeenCalledTimes(2);

		window.dispatchEvent(new Event('focus'));
		await vi.advanceTimersByTimeAsync(0);
		expect(deps.listTerminals).toHaveBeenCalledTimes(3);

		poller.dispose();
		window.dispatchEvent(new Event('focus'));
		await vi.advanceTimersByTimeAsync(5000);
		expect(deps.listTerminals).toHaveBeenCalledTimes(3);
	});
});
