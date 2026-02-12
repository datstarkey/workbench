import type {
	GitChangedEvent,
	GitHubBranchStatus,
	GitHubPR,
	GitHubProjectStatus,
	GitHubRemote
} from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export class GitHubStore {
	ghAvailable: boolean | null = $state(null);
	remoteByProject: Record<string, GitHubRemote | null> = $state({});
	prsByProject: Record<string, GitHubPR[]> = $state({});

	// Not reactive — internal bookkeeping only
	// eslint-disable-next-line svelte/prefer-svelte-reactivity
	private pendingProjects = new Set<string>();
	private pollIntervalId: ReturnType<typeof setInterval> | null = null;
	private activeBranches: Array<{ projectPath: string; branch: string }> = [];
	// eslint-disable-next-line svelte/prefer-svelte-reactivity
	private debounceTimers = new Map<string, ReturnType<typeof setTimeout>>();

	constructor() {
		listen<GitChangedEvent>('git:changed', (event) => {
			this.debouncedRefresh(event.payload.projectPath);
		});

		this.pollIntervalId = setInterval(() => {
			this.pollActiveBranches();
		}, 90_000);
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
		return { pr, remote };
	}

	getRemoteUrl(projectPath: string): string | null {
		return this.remoteByProject[projectPath]?.htmlUrl ?? null;
	}

	setActiveBranches(branches: Array<{ projectPath: string; branch: string }>) {
		this.activeBranches = branches;
	}

	async refreshProject(projectPath: string): Promise<void> {
		if (this.ghAvailable === false) return;
		await this.fetchProjectStatus(projectPath);
	}

	private async pollActiveBranches(): Promise<void> {
		if (this.ghAvailable === false) return;

		// Collect unique projects that have any open PRs
		const seen: Record<string, true> = {};
		const projectsWithOpenPrs = this.activeBranches
			.filter(({ projectPath }) => {
				if (seen[projectPath]) return false;
				const prs = this.prsByProject[projectPath];
				if (!prs?.some((p) => p.state === 'OPEN')) return false;
				seen[projectPath] = true;
				return true;
			})
			.map(({ projectPath }) => projectPath);

		for (const projectPath of projectsWithOpenPrs) {
			await this.fetchProjectStatus(projectPath);
		}
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
		for (const timer of this.debounceTimers.values()) {
			clearTimeout(timer);
		}
	}
}
