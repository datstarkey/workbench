import { describe, it, expect } from 'vitest';
import type { GitHubPR, GitHubWorkflowRun } from '$types/workbench';
import {
	checkFromRun,
	compareUrl,
	formatDuration,
	groupChecks,
	mergeMode,
	readinessRows,
	runIdFromLink
} from './pr-view';

function pr(overrides: Partial<GitHubPR> = {}): GitHubPR {
	return {
		number: 1,
		title: 'Test',
		state: 'OPEN',
		url: 'https://github.com/o/r/pull/1',
		isDraft: false,
		headRefName: 'feat',
		reviewDecision: null,
		checksStatus: { overall: 'success', total: 3, passing: 3, failing: 0, pending: 0 },
		mergeStateStatus: 'CLEAN',
		actions: { canMerge: true, canMarkReady: false, canUpdateBranch: false },
		...overrides
	};
}

describe('readinessRows', () => {
	it('reports a mergeable PR as all green', () => {
		const rows = readinessRows(pr({ reviewDecision: 'APPROVED' }));
		expect(rows.map((r) => [r.key, r.tone, r.label])).toEqual([
			['checks', 'ok', 'All 3 checks passed'],
			['review', 'ok', 'Approved'],
			['branch', 'ok', 'No conflicts']
		]);
	});

	it('explains what blocks the merge', () => {
		const rows = readinessRows(
			pr({
				checksStatus: { overall: 'failure', total: 14, passing: 12, failing: 1, pending: 1 },
				reviewDecision: 'REVIEW_REQUIRED',
				mergeStateStatus: 'BEHIND'
			})
		);
		expect(rows[0]).toEqual({
			key: 'checks',
			tone: 'err',
			label: '1 check failing',
			detail: '12 passed · 1 running'
		});
		expect(rows[1].tone).toBe('warn');
		expect(rows[2]).toMatchObject({ tone: 'warn', label: 'Behind base branch' });
	});

	it('flags conflicts as an error', () => {
		expect(readinessRows(pr({ mergeStateStatus: 'DIRTY' }))[2]).toMatchObject({
			tone: 'err',
			label: 'Merge conflicts'
		});
	});

	it('handles PRs without checks or review policy', () => {
		const rows = readinessRows(
			pr({ checksStatus: { overall: 'none', total: 0, passing: 0, failing: 0, pending: 0 } })
		);
		expect(rows[0]).toMatchObject({ tone: 'mute', label: 'No checks' });
		expect(rows[1]).toMatchObject({ tone: 'mute', label: 'No review required' });
	});
});

describe('mergeMode', () => {
	it('merges now when nothing blocks it', () => {
		expect(mergeMode(pr())).toBe('merge');
	});

	it('offers auto-merge when blocked by reviews or running checks', () => {
		expect(mergeMode(pr({ mergeStateStatus: 'BLOCKED' }))).toBe('auto-merge');
		expect(
			mergeMode(
				pr({
					actions: { canMerge: false, canMarkReady: false, canUpdateBranch: false },
					mergeStateStatus: 'BLOCKED'
				})
			)
		).toBe('auto-merge');
	});

	it('asks drafts to be marked ready', () => {
		expect(
			mergeMode(
				pr({
					isDraft: true,
					actions: { canMerge: false, canMarkReady: true, canUpdateBranch: false }
				})
			)
		).toBe('ready-for-review');
	});

	it('has no action for conflicts or closed PRs', () => {
		expect(mergeMode(pr({ mergeStateStatus: 'DIRTY' }))).toBeNull();
		expect(mergeMode(pr({ state: 'MERGED' }))).toBeNull();
	});
});

describe('groupChecks', () => {
	it('buckets checks by outcome', () => {
		const groups = groupChecks([
			{ bucket: 'pass' as const },
			{ bucket: 'fail' as const },
			{ bucket: 'pending' as const },
			{ bucket: 'skipping' as const },
			{ bucket: 'cancel' as const }
		]);
		expect(groups.failing).toHaveLength(1);
		expect(groups.pending).toHaveLength(1);
		expect(groups.passed).toHaveLength(1);
		expect(groups.skipped).toHaveLength(2);
	});
});

describe('runIdFromLink', () => {
	it('extracts the Actions run id', () => {
		expect(runIdFromLink('https://github.com/o/r/actions/runs/123456/job/789')).toBe(123456);
	});

	it('returns null for non-Actions checks', () => {
		expect(runIdFromLink('https://vercel.com/o/r/deploy')).toBeNull();
	});
});

describe('checkFromRun', () => {
	const run: GitHubWorkflowRun = {
		id: 42,
		name: 'CI',
		displayTitle: 'wip: thing',
		headBranch: 'feat',
		status: 'completed',
		conclusion: 'failure',
		url: 'https://github.com/o/r/actions/runs/42',
		event: 'push',
		createdAt: '2026-09-24T10:00:00Z',
		updatedAt: '2026-09-24T10:06:40Z'
	};

	it('maps run outcome and keeps the run id', () => {
		expect(checkFromRun(run)).toMatchObject({
			name: 'CI',
			bucket: 'fail',
			runId: 42,
			subtitle: 'push · wip: thing'
		});
		expect(checkFromRun({ ...run, status: 'in_progress', conclusion: null }).bucket).toBe(
			'pending'
		);
		expect(checkFromRun({ ...run, conclusion: 'cancelled' }).bucket).toBe('cancel');
	});
});

describe('formatDuration', () => {
	it('formats elapsed time', () => {
		expect(formatDuration('2026-09-24T10:00:00Z', '2026-09-24T10:06:40Z')).toBe('6m 40s');
		expect(formatDuration('2026-09-24T10:00:00Z', '2026-09-24T10:00:52Z')).toBe('52s');
		expect(formatDuration('2026-09-24T10:00:00Z', null)).toBeNull();
	});
});

describe('compareUrl', () => {
	it('encodes the branch', () => {
		expect(compareUrl('https://github.com/o/r', 'feat/x')).toBe(
			'https://github.com/o/r/compare/feat%2Fx?expand=1'
		);
	});
});
