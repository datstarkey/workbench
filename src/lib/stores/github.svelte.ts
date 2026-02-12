import type { GitChangedEvent, GitHubBranchStatus, GitHubRemote } from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { SvelteSet } from 'svelte/reactivity';

export class GitHubStore {
	ghAvailableByProject: Record<string, boolean> = $state({});
	remoteByProject: Record<string, GitHubRemote | null> = $state({});
	branchStatusByKey: Record<string, GitHubBranchStatus> = $state({});

	private pendingKeys = new SvelteSet<string>();
	private pollIntervalId: ReturnType<typeof setInterval> | null = null;
	private activeBranches: Array<{ projectPath: string; branch: string }> = [];

	constructor() {
		listen<GitChangedEvent>('git:changed', (event) => {
			this.refreshProject(event.payload.projectPath);
		});

		this.pollIntervalId = setInterval(() => {
			this.pollActiveBranches();
		}, 90_000);
	}

	private static branchKey(projectPath: string, branch: string): string {
		return `${projectPath}::${branch}`;
	}

	async checkGhAvailable(projectPath: string): Promise<boolean> {
		if (projectPath in this.ghAvailableByProject) {
			return this.ghAvailableByProject[projectPath];
		}
		try {
			const available = await invoke<boolean>('github_is_available', { path: projectPath });
			this.ghAvailableByProject = { ...this.ghAvailableByProject, [projectPath]: available };
			return available;
		} catch {
			this.ghAvailableByProject = { ...this.ghAvailableByProject, [projectPath]: false };
			return false;
		}
	}

	async fetchRemote(projectPath: string): Promise<GitHubRemote | null> {
		try {
			const remote = await invoke<GitHubRemote>('github_get_remote', { path: projectPath });
			this.remoteByProject = { ...this.remoteByProject, [projectPath]: remote };
			return remote;
		} catch {
			this.remoteByProject = { ...this.remoteByProject, [projectPath]: null };
			return null;
		}
	}

	async fetchBranchStatus(projectPath: string, branch: string): Promise<void> {
		const key = GitHubStore.branchKey(projectPath, branch);
		if (this.pendingKeys.has(key)) return;
		this.pendingKeys.add(key);

		try {
			const status = await invoke<GitHubBranchStatus>('github_branch_status', {
				projectPath,
				branch
			});
			this.branchStatusByKey = { ...this.branchStatusByKey, [key]: status };

			if (status.remote) {
				this.remoteByProject = { ...this.remoteByProject, [projectPath]: status.remote };
			}
		} catch (e) {
			console.warn('[GitHubStore] Failed to fetch branch status:', e);
		} finally {
			this.pendingKeys.delete(key);
		}
	}

	getBranchStatus(projectPath: string, branch: string): GitHubBranchStatus | undefined {
		return this.branchStatusByKey[GitHubStore.branchKey(projectPath, branch)];
	}

	getRemoteUrl(projectPath: string): string | null {
		return this.remoteByProject[projectPath]?.htmlUrl ?? null;
	}

	setActiveBranches(branches: Array<{ projectPath: string; branch: string }>) {
		this.activeBranches = branches;
	}

	async refreshProject(projectPath: string): Promise<void> {
		const available = await this.checkGhAvailable(projectPath);
		if (!available) return;

		const branchesToRefresh = this.activeBranches.filter((b) => b.projectPath === projectPath);
		await Promise.all(
			branchesToRefresh.map((b) => this.fetchBranchStatus(b.projectPath, b.branch))
		);
	}

	private async pollActiveBranches(): Promise<void> {
		for (const { projectPath, branch } of this.activeBranches) {
			const available = this.ghAvailableByProject[projectPath];
			if (available === false) continue;
			const key = GitHubStore.branchKey(projectPath, branch);
			const existing = this.branchStatusByKey[key];
			if (existing?.pr) {
				await this.fetchBranchStatus(projectPath, branch);
			}
		}
	}

	async initForProjects(projectPaths: string[]): Promise<void> {
		await Promise.all(projectPaths.map((p) => this.checkGhAvailable(p)));
	}

	destroy() {
		if (this.pollIntervalId) {
			clearInterval(this.pollIntervalId);
		}
	}
}
