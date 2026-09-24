import { describe, it, expect } from 'vitest';
import type { BranchInfo, GitStatusResult } from '$types/workbench';
import {
	branchOptions,
	commitButtonLabel,
	splitPath,
	subjectLength,
	syncButtonLabel,
	syncState,
	syncSummary
} from './git-view';

function status(overrides: Partial<GitStatusResult> = {}): GitStatusResult {
	return { branch: 'main', files: [], ahead: 0, behind: 0, hasUpstream: true, ...overrides };
}

describe('splitPath', () => {
	it('splits name from dir', () => {
		expect(splitPath('src/lib/a.ts')).toEqual({ name: 'a.ts', dir: 'src/lib' });
		expect(splitPath('README.md')).toEqual({ name: 'README.md', dir: '' });
	});

	it('shows the new path of a rename', () => {
		expect(splitPath('old/a.ts -> new/b.ts')).toEqual({ name: 'b.ts', dir: 'new' });
	});
});

describe('syncState', () => {
	it('maps ahead/behind/upstream to one action', () => {
		expect(syncState(status({ hasUpstream: false }))).toEqual({ kind: 'publish' });
		expect(syncState(status({ ahead: 2 }))).toEqual({ kind: 'push', ahead: 2 });
		expect(syncState(status({ behind: 3 }))).toEqual({ kind: 'pull', behind: 3 });
		expect(syncState(status({ ahead: 1, behind: 1 }))).toEqual({
			kind: 'diverged',
			ahead: 1,
			behind: 1
		});
		expect(syncState(status())).toEqual({ kind: 'synced' });
	});

	it('treats detached HEAD as unsyncable, not unpublished', () => {
		expect(syncState(status({ branch: 'HEAD', hasUpstream: false }))).toEqual({
			kind: 'detached'
		});
	});
});

describe('syncSummary / syncButtonLabel', () => {
	it('describes the state and labels the button', () => {
		const push = syncState(status({ ahead: 1 }));
		expect(syncSummary(push)).toBe('1 commit not pushed');
		expect(syncButtonLabel(push)).toBe('Push 1');

		const pull = syncState(status({ behind: 3 }));
		expect(syncSummary(pull)).toBe('3 new commits on remote');
		expect(syncButtonLabel(pull)).toBe('Pull 3');

		const diverged = syncState(status({ ahead: 2, behind: 1 }));
		expect(syncSummary(diverged)).toBe('Diverged: 1 behind, 2 ahead');
		expect(syncButtonLabel(diverged)).toBeNull();

		const synced = syncState(status());
		expect(syncSummary(synced)).toBe('Up to date with remote');
		expect(syncButtonLabel(synced)).toBeNull();
	});
});

describe('commitButtonLabel', () => {
	it('says what will happen', () => {
		expect(commitButtonLabel(3, false)).toBe('Commit 3 staged files');
		expect(commitButtonLabel(1, false)).toBe('Commit 1 staged file');
		expect(commitButtonLabel(0, false)).toBe('Stage files to commit');
		expect(commitButtonLabel(0, true)).toBe('Amend last commit');
	});
});

describe('branchOptions', () => {
	const branches: BranchInfo[] = [
		{ name: 'main', sha: 'a1', isCurrent: true, isRemote: false },
		{ name: 'fix/one', sha: 'b2', isCurrent: false, isRemote: false },
		{ name: 'fix/two', sha: 'c3', isCurrent: false, isRemote: false },
		{ name: 'origin/fix/one', sha: 'b2', isCurrent: false, isRemote: true },
		{ name: 'origin/fix/three', sha: 'd4', isCurrent: false, isRemote: true }
	];

	it('omits the current branch and remotes already tracked locally', () => {
		const opts = branchOptions(branches, [], '');
		expect(opts.local.map((o) => o.name)).toEqual(['fix/one', 'fix/two']);
		expect(opts.remote.map((o) => o.name)).toEqual(['origin/fix/three']);
		expect(opts.createName).toBeNull();
	});

	it('checks out remotes by their short name', () => {
		expect(branchOptions(branches, [], '').remote[0].checkoutName).toBe('fix/three');
	});

	it('flags branches checked out in another worktree', () => {
		const opts = branchOptions(branches, ['fix/two'], '');
		expect(opts.local.find((o) => o.name === 'fix/two')?.inWorktree).toBe(true);
		expect(opts.local.find((o) => o.name === 'fix/one')?.inWorktree).toBe(false);
	});

	it('filters case-insensitively and offers to create a new name', () => {
		const opts = branchOptions(branches, [], ' FIX/T ');
		expect(opts.local.map((o) => o.name)).toEqual(['fix/two']);
		expect(opts.remote.map((o) => o.name)).toEqual(['origin/fix/three']);
		expect(opts.createName).toBe('FIX/T');
	});

	it('does not offer to create an existing local branch', () => {
		expect(branchOptions(branches, [], 'fix/one').createName).toBeNull();
	});

	it('does not offer to create a name that exists on a remote', () => {
		expect(branchOptions(branches, [], 'fix/three').createName).toBeNull();
		expect(branchOptions(branches, [], 'origin/fix/three').createName).toBeNull();
	});
});

describe('subjectLength', () => {
	it('counts only the first line', () => {
		expect(subjectLength('feat: x\n\nbody text')).toBe(7);
	});
});
