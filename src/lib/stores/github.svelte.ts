import type {
	GitChangedEvent,
	GitHubBranchRuns,
	GitHubBranchStatus,
	GitHubCheckDetail,
	GitHubPR,
	GitHubProjectStatus,
	GitHubRemote
} from '$types/workbench';
import { getClaudeSessionStore, getGitStore, getProjectStore, getWorkspaceStore } from './context';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { load } from '@tauri-apps/plugin-store';

const FAST_POLL_MS = 15_000;
const SLOW_POLL_MS = 90_000;

export class GitHubStore {
	private workspaces = getWorkspaceStore();
	private git = getGitStore();
	private sessions = getClaudeSessionStore();
	private projects = getProjectStore();

	ghAvailable: boolean | null = $state(null);
	remoteByProject: Record<string, GitHubRemote | null> = $state({});
	prsByProject: Record<string, GitHubPR[]> = $state({});
	branchRunsByProject: Record<string, Record<string, GitHubBranchRuns>> = $state({});

	// Sidebar state
	sidebarOpen: boolean = $state(false);
	private _overrideTarget: { projectPath: string; branch: string } | null = $state(null);
	private _overrideForWorkspaceId: string | null = $state(null);
	checksByPr: Record<string, GitHubCheckDetail[]> = $state({});

	/** Derived sidebar target: override (valid only for current workspace) or active workspace */
	readonly sidebarTarget = $derived.by(() => {
		if (this._overrideTarget && this._overrideForWorkspaceId === this.workspaces.activeWorkspaceId)
			return this._overrideTarget;
		const ws = this.workspaces.activeWorkspace;
		if (!ws) return null;
		const branch = ws.branch ?? this.git.branchByProject[ws.projectPath] ?? null;
		if (!branch) return null;
		return { projectPath: ws.projectPath, branch };
	});

	/** PR matching the current sidebar target branch */
	readonly sidebarPr = $derived.by((): GitHubPR | null => {
		const target = this.sidebarTarget;
		if (!target) return null;
		const prs = this.prsByProject[target.projectPath] ?? [];
		return prs.find((p) => p.headRefName === target.branch) ?? null;
	});

	/** Branch workflow runs for the current sidebar target */
	readonly sidebarBranchRuns = $derived.by((): GitHubBranchRuns | null => {
		const target = this.sidebarTarget;
		if (!target) return null;
		return this.branchRunsByProject[target.projectPath]?.[target.branch] ?? null;
	});

	/** PR checks for the current sidebar PR */
	readonly sidebarChecks = $derived.by((): GitHubCheckDetail[] => {
		const target = this.sidebarTarget;
		const pr = this.sidebarPr;
		if (!target || !pr) return [];
		return this.checksByPr[this.prKey(target.projectPath, pr.number)] ?? [];
	});

	/** Derived list of branches with active AI sessions — drives polling */
	readonly activeBranches = $derived.by(() => {
		if (this.ghAvailable === false) return [];
		const sessionsByProject = this.sessions.activeSessionsByProject;
		const branches: Array<{ projectPath: string; branch: string }> = [];
		for (const project of this.projects.projects) {
			const hasSessions = (sessionsByProject[project.path]?.length ?? 0) > 0;
			if (!hasSessions) continue;
			const branch = this.git.branchByProject[project.path];
			if (branch) {
				branches.push({ projectPath: project.path, branch });
			}
			const worktrees = (this.git.worktreesByProject[project.path] ?? []).filter(
				(wt) => !wt.isMain
			);
			for (const wt of worktrees) {
				if (wt.branch) {
					branches.push({ projectPath: project.path, branch: wt.branch });
				}
			}
		}
		return branches;
	});

	// Not reactive — internal bookkeeping only
	// eslint-disable-next-line svelte/prefer-svelte-reactivity
	private pendingProjects = new Set<string>();
	private pollIntervalId: ReturnType<typeof setInterval> | null = null;
	// eslint-disable-next-line svelte/prefer-svelte-reactivity
	private debounceTimers = new Map<string, ReturnType<typeof setTimeout>>();
	private fastPollIntervalId: ReturnType<typeof setInterval> | null = null;
	// eslint-disable-next-line svelte/prefer-svelte-reactivity
	private pendingChecks = new Set<string>();

	constructor() {
		listen<GitChangedEvent>('git:changed', (event) => {
			this.debouncedRefresh(event.payload.projectPath);
		});

		this.pollIntervalId = setInterval(() => {
			this.pollActiveBranches();
		}, SLOW_POLL_MS);
	}

	private debouncedRefresh(projectPath: string): void {
		const existing = this.debounceTimers.get(projectPath);
		if (existing) clearTimeout(existing);
		this.debounceTimers.set(
			projectPath,
			setTimeout(() => {
				this.debounceTimers.delete(projectPath);
				this.refreshProject(projectPath);
			}, 2_000)
		);
	}

	async checkGhAvailable(): Promise<boolean> {
		if (this.ghAvailable !== null) return this.ghAvailable;
		try {
			const available = await invoke<boolean>('github_is_available');
			this.ghAvailable = available;
			return available;
		} catch {
			this.ghAvailable = false;
			return false;
		}
	}

	async fetchProjectStatus(projectPath: string): Promise<void> {
		if (this.pendingProjects.has(projectPath)) return;
		this.pendingProjects.add(projectPath);

		try {
			const status = await invoke<GitHubProjectStatus>('github_project_status', {
				projectPath
			});
			this.remoteByProject = { ...this.remoteByProject, [projectPath]: status.remote };
			this.prsByProject = { ...this.prsByProject, [projectPath]: status.prs };
			this.branchRunsByProject = {
				...this.branchRunsByProject,
				[projectPath]: status.branchRuns
			};
			// Merge pre-fetched PR checks
			const newChecks = { ...this.checksByPr };
			for (const [prNum, checks] of Object.entries(status.prChecks)) {
				newChecks[`${projectPath}::${prNum}`] = checks;
			}
			this.checksByPr = newChecks;
			this.updateFastPolling();
		} catch (e) {
			console.warn('[GitHubStore] Failed to fetch project status:', e);
		} finally {
			this.pendingProjects.delete(projectPath);
		}
	}

	getBranchStatus(projectPath: string, branch: string): GitHubBranchStatus | undefined {
		const prs = this.prsByProject[projectPath];
		if (!prs) return undefined;
		const pr = prs.find((p) => p.headRefName === branch) ?? null;
		const remote = this.remoteByProject[projectPath] ?? null;
		const branchRuns = this.branchRunsByProject[projectPath]?.[branch] ?? null;
		return { pr, remote, branchRuns };
	}

	getRemoteUrl(projectPath: string): string | null {
		return this.remoteByProject[projectPath]?.htmlUrl ?? null;
	}

	async refreshProject(projectPath: string): Promise<void> {
		if (this.ghAvailable === false) return;
		await this.fetchProjectStatus(projectPath);
	}

	private async pollActiveBranches(): Promise<void> {
		if (this.ghAvailable === false) return;

		// Poll all projects with visible branches — not just those with existing PRs.
		// This ensures newly created PRs are discovered within one poll cycle.
		const seen: Record<string, true> = {};
		const uniqueProjects = this.activeBranches
			.filter(({ projectPath }) => {
				if (seen[projectPath]) return false;
				seen[projectPath] = true;
				return true;
			})
			.map(({ projectPath }) => projectPath);

		for (const projectPath of uniqueProjects) {
			await this.fetchProjectStatus(projectPath);
		}
	}

	// --- PR checks / sidebar ---

	private prKey(projectPath: string, prNumber: number): string {
		return `${projectPath}::${prNumber}`;
	}

	async fetchPrChecks(projectPath: string, prNumber: number): Promise<void> {
		const key = this.prKey(projectPath, prNumber);
		if (this.pendingChecks.has(key)) return;
		this.pendingChecks.add(key);

		try {
			const checks = await invoke<GitHubCheckDetail[]>('github_pr_checks', {
				projectPath,
				prNumber
			});
			this.checksByPr = { ...this.checksByPr, [key]: checks };

			// Manage adaptive polling based on check states
			this.updateFastPolling();
		} catch (e) {
			console.warn('[GitHubStore] Failed to fetch PR checks:', e);
		} finally {
			this.pendingChecks.delete(key);
		}
	}

	getPrChecks(projectPath: string, prNumber: number): GitHubCheckDetail[] | undefined {
		return this.checksByPr[this.prKey(projectPath, prNumber)];
	}

	showBranch(projectPath: string, branch: string): void {
		this._overrideTarget = { projectPath, branch };
		this._overrideForWorkspaceId = this.workspaces.activeWorkspaceId;
		this.refreshProject(projectPath);
	}

	clearSidebarOverride(): void {
		this._overrideTarget = null;
		this._overrideForWorkspaceId = null;
		this.stopFastPolling();
	}

	async toggleSidebar(): Promise<void> {
		this.sidebarOpen = !this.sidebarOpen;
		await this.persistSidebarState();
	}

	async initSidebarState(): Promise<void> {
		try {
			const store = await load('ui-state.json');
			const open = await store.get<boolean>('githubSidebarOpen');
			if (typeof open === 'boolean') {
				this.sidebarOpen = open;
			}
		} catch {
			// Ignore — first run, no persisted state
		}
	}

	private async persistSidebarState(): Promise<void> {
		try {
			const store = await load('ui-state.json');
			await store.set('githubSidebarOpen', this.sidebarOpen);
			await store.save();
		} catch {
			// Non-critical
		}
	}

	private updateFastPolling(): void {
		const target = this.sidebarTarget;
		if (!target) {
			this.stopFastPolling();
			return;
		}

		let hasPending = false;
		const pr = this.sidebarPr;
		if (pr) {
			const checks = this.checksByPr[this.prKey(target.projectPath, pr.number)];
			hasPending = checks?.some((c) => c.bucket === 'pending') ?? false;
		} else {
			const runs = this.branchRunsByProject[target.projectPath]?.[target.branch];
			hasPending = runs?.status.pending > 0;
		}

		if (hasPending && !this.fastPollIntervalId) {
			this.startFastPolling();
		} else if (!hasPending && this.fastPollIntervalId) {
			this.stopFastPolling();
		}
	}

	private startFastPolling(): void {
		this.stopFastPolling();
		this.fastPollIntervalId = setInterval(() => {
			this.fastPollTick();
		}, FAST_POLL_MS);
	}

	private stopFastPolling(): void {
		if (this.fastPollIntervalId) {
			clearInterval(this.fastPollIntervalId);
			this.fastPollIntervalId = null;
		}
	}

	private fastPollTick(): void {
		const target = this.sidebarTarget;
		if (!target) {
			this.stopFastPolling();
			return;
		}
		const pr = this.sidebarPr;
		if (pr) {
			this.fetchPrChecks(target.projectPath, pr.number);
		}
		this.refreshProject(target.projectPath);
	}

	/** Call after projects load — checks gh availability, then fetches status per project */
	async initForProjects(projectPaths: string[]): Promise<void> {
		const available = await this.checkGhAvailable();
		if (!available) return;

		await Promise.all(projectPaths.map((p) => this.fetchProjectStatus(p)));
	}

	destroy() {
		if (this.pollIntervalId) {
			clearInterval(this.pollIntervalId);
		}
		this.stopFastPolling();
		for (const timer of this.debounceTimers.values()) {
			clearTimeout(timer);
		}
	}
}
