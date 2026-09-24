import type {
	GitHubCheckDetail,
	GitHubChecksStatus,
	GitHubPR,
	GitHubWorkflowRun
} from '$types/workbench';
import { plural } from '$lib/utils/format';

export type Tone = 'ok' | 'warn' | 'err' | 'mute';

export const TONE_TEXT: Record<Tone, string> = {
	ok: 'text-wb-ok',
	warn: 'text-wb-warn',
	err: 'text-wb-err',
	mute: 'text-wb-ink-soft'
};

export const TONE_BG: Record<Tone, string> = {
	ok: 'bg-wb-ok',
	warn: 'bg-wb-warn',
	err: 'bg-wb-err',
	mute: 'bg-wb-ink-soft'
};

export function checksTone(overall: GitHubChecksStatus['overall']): Exclude<Tone, 'mute'> | null {
	switch (overall) {
		case 'success':
			return 'ok';
		case 'failure':
			return 'err';
		case 'pending':
			return 'warn';
		default:
			return null;
	}
}

export function prStateLabel(pr: GitHubPR): { label: string; class: string } {
	if (pr.state === 'MERGED') return { label: 'Merged', class: 'bg-purple-400/15 text-purple-400' };
	if (pr.state === 'CLOSED') return { label: 'Closed', class: 'bg-wb-err/10 text-wb-err' };
	if (pr.isDraft) return { label: 'Draft', class: 'bg-wb-panel2 text-wb-ink-mute' };
	return { label: 'Open', class: 'bg-wb-ok/15 text-wb-ok' };
}

export interface ReadinessRow {
	key: 'checks' | 'review' | 'branch';
	tone: Tone;
	label: string;
	detail: string | null;
}

function checksRow({ total, passing, failing, pending }: GitHubChecksStatus): ReadinessRow {
	const detail = (parts: string[]) => parts.filter(Boolean).join(' · ') || null;
	if (total === 0) return { key: 'checks', tone: 'mute', label: 'No checks', detail: null };
	if (failing > 0) {
		return {
			key: 'checks',
			tone: 'err',
			label: `${plural(failing, 'check')} failing`,
			detail: detail([passing ? `${passing} passed` : '', pending ? `${pending} running` : ''])
		};
	}
	if (pending > 0) {
		return {
			key: 'checks',
			tone: 'warn',
			label: `${plural(pending, 'check')} running`,
			detail: detail([passing ? `${passing} passed` : ''])
		};
	}
	return {
		key: 'checks',
		tone: 'ok',
		label: total === 1 ? 'Check passed' : `All ${total} checks passed`,
		detail: null
	};
}

function reviewRow(decision: GitHubPR['reviewDecision']): ReadinessRow {
	switch (decision) {
		case 'APPROVED':
			return { key: 'review', tone: 'ok', label: 'Approved', detail: null };
		case 'CHANGES_REQUESTED':
			return { key: 'review', tone: 'err', label: 'Changes requested', detail: null };
		case 'REVIEW_REQUIRED':
			return { key: 'review', tone: 'warn', label: 'Review required', detail: null };
		default:
			return { key: 'review', tone: 'mute', label: 'No review required', detail: null };
	}
}

function branchRow(state: GitHubPR['mergeStateStatus']): ReadinessRow {
	if (state === 'DIRTY') {
		return {
			key: 'branch',
			tone: 'err',
			label: 'Merge conflicts',
			detail: 'Resolve them locally, then push'
		};
	}
	if (state === 'BEHIND') {
		return {
			key: 'branch',
			tone: 'warn',
			label: 'Behind base branch',
			detail: 'Base has new commits'
		};
	}
	return { key: 'branch', tone: 'ok', label: 'No conflicts', detail: null };
}

/** The three things that decide whether an open PR can merge. */
export function readinessRows(pr: GitHubPR): ReadinessRow[] {
	return [checksRow(pr.checksStatus), reviewRow(pr.reviewDecision), branchRow(pr.mergeStateStatus)];
}

/** GitHub states where merging right now is expected to succeed. */
const MERGEABLE_NOW = new Set<GitHubPR['mergeStateStatus']>(['CLEAN', 'HAS_HOOKS', 'UNSTABLE']);

/**
 * Primary action for an open PR: mark a draft ready, merge now when nothing blocks it,
 * otherwise offer auto-merge (GitHub merges once checks and reviews pass). Conflicts
 * need local work, so there is no merge action.
 */
export function mergeMode(pr: GitHubPR): 'ready-for-review' | 'merge' | 'auto-merge' | null {
	if (pr.state !== 'OPEN') return null;
	if (pr.actions.canMarkReady) return 'ready-for-review';
	if (pr.isDraft || pr.mergeStateStatus === 'DIRTY') return null;
	if (pr.actions.canMerge && MERGEABLE_NOW.has(pr.mergeStateStatus)) return 'merge';
	return 'auto-merge';
}

export interface CheckGroups<T> {
	failing: T[];
	pending: T[];
	passed: T[];
	skipped: T[];
}

export function groupChecks<T extends Pick<GitHubCheckDetail, 'bucket'>>(
	checks: T[]
): CheckGroups<T> {
	return {
		failing: checks.filter((c) => c.bucket === 'fail'),
		pending: checks.filter((c) => c.bucket === 'pending'),
		passed: checks.filter((c) => c.bucket === 'pass'),
		skipped: checks.filter((c) => c.bucket === 'skipping' || c.bucket === 'cancel')
	};
}

/** Actions run id from a check's details link (`…/actions/runs/<id>/job/<id>`). */
export function runIdFromLink(link: string): number | null {
	const match = /\/actions\/runs\/(\d+)/.exec(link);
	return match ? Number(match[1]) : null;
}

export type RunCheck = GitHubCheckDetail & { runId: number | null; subtitle: string };

export function checkFromRun(run: GitHubWorkflowRun): RunCheck {
	let bucket: GitHubCheckDetail['bucket'];
	if (run.status !== 'completed') bucket = 'pending';
	else if (run.conclusion === 'success' || run.conclusion === 'skipped') bucket = 'pass';
	else if (run.conclusion === 'cancelled') bucket = 'cancel';
	else bucket = 'fail';
	return {
		name: run.name,
		bucket,
		workflow: run.name,
		link: run.url,
		startedAt: run.createdAt,
		completedAt: run.status === 'completed' ? run.updatedAt : null,
		description: '',
		runId: run.id,
		subtitle: `${run.event} · ${run.displayTitle}`
	};
}

export function checkFromPrCheck(check: GitHubCheckDetail): RunCheck {
	return { ...check, runId: runIdFromLink(check.link), subtitle: check.workflow };
}

export function formatDuration(
	startedAt: string | null,
	completedAt: string | null
): string | null {
	if (!startedAt || !completedAt) return null;
	const start = new Date(startedAt).getTime();
	const end = new Date(completedAt).getTime();
	if (isNaN(start) || isNaN(end) || start <= 0 || end <= 0) return null;
	const seconds = Math.round((end - start) / 1000);
	if (seconds < 0) return null;
	if (seconds < 60) return `${seconds}s`;
	const mins = Math.floor(seconds / 60);
	const secs = seconds % 60;
	return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`;
}

export function compareUrl(repoUrl: string, branch: string): string {
	return `${repoUrl}/compare/${encodeURIComponent(branch)}?expand=1`;
}

export function commitUrl(repoUrl: string, sha: string): string {
	return `${repoUrl}/commit/${sha}`;
}
