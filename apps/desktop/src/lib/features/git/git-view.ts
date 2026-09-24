import type { BranchInfo, GitFileStatus, GitStatusResult } from '$types/workbench';
import { plural } from '$lib/utils/format';

const STATUS_DISPLAY: Record<string, { letter: string; color: string }> = {
	modified: { letter: 'M', color: 'text-wb-warn' },
	added: { letter: 'A', color: 'text-wb-ok' },
	deleted: { letter: 'D', color: 'text-wb-err' },
	renamed: { letter: 'R', color: 'text-wb-codex' },
	copied: { letter: 'C', color: 'text-wb-codex' },
	untracked: { letter: 'U', color: 'text-wb-ink-soft' }
};

const FALLBACK = { letter: '?', color: 'text-wb-ink-soft' };

export function getStatusDisplay(status: string): { letter: string; color: string } {
	return STATUS_DISPLAY[status] ?? FALLBACK;
}

/** Split a repo-relative path into file name and parent dir; renames show their new path. */
export function splitPath(path: string): { name: string; dir: string } {
	const target = path.split(' -> ').pop() ?? path;
	const i = target.lastIndexOf('/');
	return i === -1
		? { name: target, dir: '' }
		: { name: target.slice(i + 1), dir: target.slice(0, i) };
}

export function stagedFiles(files: GitFileStatus[]): GitFileStatus[] {
	return files.filter((f) => f.staged);
}

export function unstagedFiles(files: GitFileStatus[]): GitFileStatus[] {
	return files.filter((f) => !f.staged || f.unstaged);
}

export type SyncState =
	| { kind: 'detached' }
	| { kind: 'publish' }
	| { kind: 'push'; ahead: number }
	| { kind: 'pull'; behind: number }
	| { kind: 'diverged'; ahead: number; behind: number }
	| { kind: 'synced' };

export function syncState(status: GitStatusResult): SyncState {
	if (status.branch === 'HEAD') return { kind: 'detached' };
	if (!status.hasUpstream) return { kind: 'publish' };
	const { ahead, behind } = status;
	// A plain pull can't reconcile divergence without a rebase/merge choice, so no one-click action
	if (ahead > 0 && behind > 0) return { kind: 'diverged', ahead, behind };
	if (ahead > 0) return { kind: 'push', ahead };
	if (behind > 0) return { kind: 'pull', behind };
	return { kind: 'synced' };
}

export function syncSummary(state: SyncState): string {
	switch (state.kind) {
		case 'detached':
			return 'Detached HEAD';
		case 'publish':
			return 'Branch not published';
		case 'push':
			return `${plural(state.ahead, 'commit')} not pushed`;
		case 'pull':
			return `${plural(state.behind, 'new commit')} on remote`;
		case 'diverged':
			return `Diverged: ${state.behind} behind, ${state.ahead} ahead`;
		case 'synced':
			return 'Up to date with remote';
	}
}

/** Label for the single sync button, or null when there is nothing to sync. */
export function syncButtonLabel(state: SyncState): string | null {
	switch (state.kind) {
		case 'publish':
			return 'Publish';
		case 'push':
			return `Push ${state.ahead}`;
		case 'pull':
			return `Pull ${state.behind}`;
		default:
			return null;
	}
}

export function commitButtonLabel(stagedCount: number, amend: boolean): string {
	if (amend) return 'Amend last commit';
	if (stagedCount === 0) return 'Stage files to commit';
	return `Commit ${plural(stagedCount, 'staged file')}`;
}

export interface BranchOption {
	/** Name as listed by git (`origin/foo` for remotes) */
	name: string;
	/** Name to pass to `git checkout` (remote prefix stripped so git creates a tracking branch) */
	checkoutName: string;
	sha: string;
	isRemote: boolean;
	/** Checked out in another worktree, so git refuses to check it out here */
	inWorktree: boolean;
}

export interface BranchOptions {
	local: BranchOption[];
	remote: BranchOption[];
	/** Trimmed query when it names a branch that doesn't exist yet */
	createName: string | null;
}

export function branchOptions(
	branches: BranchInfo[],
	worktreeBranches: string[],
	query: string
): BranchOptions {
	const q = query.trim();
	const needle = q.toLowerCase();
	const matches = (name: string) => name.toLowerCase().includes(needle);
	const localNames = new Set(branches.filter((b) => !b.isRemote).map((b) => b.name));

	const local = branches
		.filter((b) => !b.isRemote && !b.isCurrent && matches(b.name))
		.map((b) => ({
			name: b.name,
			checkoutName: b.name,
			sha: b.sha,
			isRemote: false,
			inWorktree: worktreeBranches.includes(b.name)
		}));

	const remote = branches
		.filter((b) => b.isRemote && matches(b.name))
		.map((b) => ({ b, short: b.name.slice(b.name.indexOf('/') + 1) }))
		.filter(({ short }) => !localNames.has(short))
		.map(({ b, short }) => ({
			name: b.name,
			checkoutName: short,
			sha: b.sha,
			isRemote: true,
			inWorktree: false
		}));

	// A name that already exists anywhere (local, `origin/foo`, or remote-only `foo`) is checked
	// out from the list instead; creating it would fork from HEAD or make refs ambiguous.
	const taken = branches.some(
		(b) => b.name === q || (b.isRemote && b.name.slice(b.name.indexOf('/') + 1) === q)
	);
	return { local, remote, createName: q && !taken ? q : null };
}

/** Length of the commit subject line; conventional limit is 72. */
export function subjectLength(message: string): number {
	return message.split('\n', 1)[0].length;
}
