import type {
	GitCommitFile,
	GitCommitResult,
	GitLogEntry,
	GitStashEntry,
	GitStatusResult,
	ProjectRefreshRequestedEvent,
	WorktreeInfo
} from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export class GitStore {
	worktreesByProject: Record<string, WorktreeInfo[]> = $state({});
	branchByProject: Record<string, string> = $state({});
	statusByProject: Record<string, GitStatusResult> = $state({});
	logByProject: Record<string, GitLogEntry[]> = $state({});
	stashByProject: Record<string, GitStashEntry[]> = $state({});

	// Non-reactive concurrency/staleness bookkeeping
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- internal bookkeeping only
	private refreshInFlight = new Set<string>();
	lastRefreshedAt: Record<string, number> = {};

	constructor() {
		listen<ProjectRefreshRequestedEvent>('project:refresh-requested', (event) => {
			void this.refreshGitState(event.payload.projectPath);
		});
	}

	private async tryInvoke<T>(
		command: string,
		args: Record<string, unknown>,
		warnPrefix: string
	): Promise<T | undefined> {
		try {
			return await invoke<T>(command, args);
		} catch (e) {
			console.warn(warnPrefix, e);
			return undefined;
		}
	}

	async fetchGitInfo(projectPath: string) {
		const info = await this.tryInvoke<{ branch: string; repoRoot: string; isWorktree: boolean }>(
			'git_info',
			{ path: projectPath },
			'[GitStore] Not a git repo or git info failed:'
		);
		if (info) {
			this.branchByProject = { ...this.branchByProject, [projectPath]: info.branch };
		}
	}

	async fetchWorktrees(projectPath: string) {
		const wts = await this.tryInvoke<WorktreeInfo[]>(
			'list_worktrees',
			{ path: projectPath },
			'[GitStore] Failed to list worktrees:'
		);
		if (wts) {
			this.worktreesByProject = { ...this.worktreesByProject, [projectPath]: wts };
		}
	}

	async fetchStatus(projectPath: string) {
		const status = await this.tryInvoke<GitStatusResult>(
			'git_status',
			{ path: projectPath },
			'[GitStore] Failed to fetch status:'
		);
		if (status) {
			this.statusByProject = { ...this.statusByProject, [projectPath]: status };
		}
	}

	async fetchLog(projectPath: string, maxCount = 20) {
		const log = await this.tryInvoke<GitLogEntry[]>(
			'git_log',
			{ path: projectPath, maxCount },
			'[GitStore] Failed to fetch log:'
		);
		if (log) {
			this.logByProject = { ...this.logByProject, [projectPath]: log };
		}
	}

	async fetchStashes(projectPath: string) {
		const stashes = await this.tryInvoke<GitStashEntry[]>(
			'git_stash_list',
			{ path: projectPath },
			'[GitStore] Failed to fetch stashes:'
		);
		if (stashes) {
			this.stashByProject = { ...this.stashByProject, [projectPath]: stashes };
		}
	}

	async stageFiles(projectPath: string, files: string[]) {
		const ok = await this.tryInvoke<void>(
			'git_stage',
			{ path: projectPath, files },
			'[GitStore] Failed to stage files:'
		);
		if (ok !== undefined) {
			await this.fetchStatus(projectPath);
		}
	}

	async unstageFiles(projectPath: string, files: string[]) {
		const ok = await this.tryInvoke<void>(
			'git_unstage',
			{ path: projectPath, files },
			'[GitStore] Failed to unstage files:'
		);
		if (ok !== undefined) {
			await this.fetchStatus(projectPath);
		}
	}

	/** Throws with git's message (e.g. a failing pre-commit hook) so the UI can show it. */
	async commit(projectPath: string, message: string): Promise<GitCommitResult> {
		const result = await invoke<GitCommitResult>('git_commit', { path: projectPath, message });
		await Promise.all([this.fetchStatus(projectPath), this.fetchLog(projectPath)]);
		return result;
	}

	async checkoutBranch(projectPath: string, branch: string) {
		await invoke('git_checkout_branch', { path: projectPath, branch });
		await this.refreshGitState(projectPath);
	}

	// Stash/discard throw so the UI can report git's error (e.g. a pop that conflicts)
	async stashPush(projectPath: string, message?: string) {
		await invoke('git_stash_push', { path: projectPath, message });
		await Promise.all([this.fetchStatus(projectPath), this.fetchStashes(projectPath)]);
	}

	async stashPop(projectPath: string, index: number) {
		try {
			await invoke('git_stash_pop', { path: projectPath, index });
		} finally {
			// A conflicting pop still changes the working tree
			await Promise.all([this.fetchStatus(projectPath), this.fetchStashes(projectPath)]);
		}
	}

	async stashDrop(projectPath: string, index: number) {
		await invoke('git_stash_drop', { path: projectPath, index });
		await this.fetchStashes(projectPath);
	}

	async discardFile(projectPath: string, file: string) {
		await invoke('git_discard_file', { path: projectPath, file });
		await this.fetchStatus(projectPath);
	}

	async fetch(projectPath: string) {
		await invoke('git_fetch', { path: projectPath });
		await this.fetchStatus(projectPath);
	}

	async pull(projectPath: string) {
		await invoke('git_pull', { path: projectPath });
		await Promise.all([
			this.fetchStatus(projectPath),
			this.fetchLog(projectPath),
			this.fetchGitInfo(projectPath)
		]);
	}

	async push(projectPath: string, setUpstream = false) {
		await invoke('git_push', { path: projectPath, setUpstream });
		await this.fetchStatus(projectPath);
	}

	async refreshGitState(projectPath: string) {
		if (this.refreshInFlight.has(projectPath)) return;
		const lastRefresh = this.lastRefreshedAt[projectPath] ?? 0;
		if (Date.now() - lastRefresh < 500) return;
		this.refreshInFlight.add(projectPath);
		try {
			await Promise.all([
				this.fetchGitInfo(projectPath),
				this.fetchWorktrees(projectPath),
				this.fetchStatus(projectPath),
				this.fetchLog(projectPath),
				this.fetchStashes(projectPath)
			]);
			this.lastRefreshedAt[projectPath] = Date.now();
		} finally {
			this.refreshInFlight.delete(projectPath);
		}
	}

	async showFiles(projectPath: string, sha: string): Promise<GitCommitFile[]> {
		const files = await this.tryInvoke<GitCommitFile[]>(
			'git_show_files',
			{ path: projectPath, sha },
			'[GitStore] Failed to show files:'
		);
		return files ?? [];
	}

	async revert(projectPath: string, sha: string): Promise<GitCommitResult | null> {
		try {
			const result = await invoke<GitCommitResult>('git_revert', { path: projectPath, sha });
			await Promise.all([this.fetchStatus(projectPath), this.fetchLog(projectPath)]);
			return result;
		} catch (e) {
			console.warn('[GitStore] Failed to revert:', e);
			throw e;
		}
	}

	async createBranch(projectPath: string, name: string, checkout = true) {
		await invoke('git_create_branch', { path: projectPath, name, checkout });
		await this.refreshGitState(projectPath);
	}

	async commitAmend(projectPath: string, message: string): Promise<GitCommitResult> {
		const result = await invoke<GitCommitResult>('git_commit_amend', {
			path: projectPath,
			message
		});
		await Promise.all([this.fetchStatus(projectPath), this.fetchLog(projectPath)]);
		return result;
	}

	async refreshAll(projectPaths: string[]) {
		await Promise.all(projectPaths.map((p) => this.refreshGitState(p)));
	}
}
