import type {
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

	constructor() {
		listen<ProjectRefreshRequestedEvent>('project:refresh-requested', (event) => {
			void this.refreshGitState(event.payload.projectPath);
		});
	}

	async fetchGitInfo(projectPath: string) {
		try {
			const info = await invoke<{ branch: string; repoRoot: string; isWorktree: boolean }>(
				'git_info',
				{ path: projectPath }
			);
			this.branchByProject = { ...this.branchByProject, [projectPath]: info.branch };
		} catch (e) {
			console.warn('[GitStore] Not a git repo or git info failed:', e);
		}
	}

	async fetchWorktrees(projectPath: string) {
		try {
			const wts = await invoke<WorktreeInfo[]>('list_worktrees', { path: projectPath });
			this.worktreesByProject = { ...this.worktreesByProject, [projectPath]: wts };
		} catch (e) {
			console.warn('[GitStore] Failed to list worktrees:', e);
		}
	}

	async fetchStatus(projectPath: string) {
		try {
			const status = await invoke<GitStatusResult>('git_status', { path: projectPath });
			this.statusByProject = { ...this.statusByProject, [projectPath]: status };
		} catch (e) {
			console.warn('[GitStore] Failed to fetch status:', e);
		}
	}

	async fetchLog(projectPath: string, maxCount = 20) {
		try {
			const log = await invoke<GitLogEntry[]>('git_log', { path: projectPath, maxCount });
			this.logByProject = { ...this.logByProject, [projectPath]: log };
		} catch (e) {
			console.warn('[GitStore] Failed to fetch log:', e);
		}
	}

	async fetchStashes(projectPath: string) {
		try {
			const stashes = await invoke<GitStashEntry[]>('git_stash_list', { path: projectPath });
			this.stashByProject = { ...this.stashByProject, [projectPath]: stashes };
		} catch (e) {
			console.warn('[GitStore] Failed to fetch stashes:', e);
		}
	}

	async stageFiles(projectPath: string, files: string[]) {
		try {
			await invoke('git_stage', { path: projectPath, files });
			await this.fetchStatus(projectPath);
		} catch (e) {
			console.warn('[GitStore] Failed to stage files:', e);
		}
	}

	async unstageFiles(projectPath: string, files: string[]) {
		try {
			await invoke('git_unstage', { path: projectPath, files });
			await this.fetchStatus(projectPath);
		} catch (e) {
			console.warn('[GitStore] Failed to unstage files:', e);
		}
	}

	async commit(projectPath: string, message: string): Promise<GitCommitResult | null> {
		try {
			const result = await invoke<GitCommitResult>('git_commit', {
				path: projectPath,
				message
			});
			await Promise.all([this.fetchStatus(projectPath), this.fetchLog(projectPath)]);
			return result;
		} catch (e) {
			console.warn('[GitStore] Failed to commit:', e);
			return null;
		}
	}

	async checkoutBranch(projectPath: string, branch: string) {
		try {
			await invoke('git_checkout_branch', { path: projectPath, branch });
			await this.refreshGitState(projectPath);
		} catch (e) {
			console.warn('[GitStore] Failed to checkout branch:', e);
		}
	}

	async stashPush(projectPath: string, message?: string) {
		try {
			await invoke('git_stash_push', { path: projectPath, message });
			await Promise.all([this.fetchStatus(projectPath), this.fetchStashes(projectPath)]);
		} catch (e) {
			console.warn('[GitStore] Failed to push stash:', e);
		}
	}

	async stashPop(projectPath: string, index: number) {
		try {
			await invoke('git_stash_pop', { path: projectPath, index });
			await Promise.all([this.fetchStatus(projectPath), this.fetchStashes(projectPath)]);
		} catch (e) {
			console.warn('[GitStore] Failed to pop stash:', e);
		}
	}

	async stashDrop(projectPath: string, index: number) {
		try {
			await invoke('git_stash_drop', { path: projectPath, index });
			await this.fetchStashes(projectPath);
		} catch (e) {
			console.warn('[GitStore] Failed to drop stash:', e);
		}
	}

	async discardFile(projectPath: string, file: string) {
		try {
			await invoke('git_discard_file', { path: projectPath, file });
			await this.fetchStatus(projectPath);
		} catch (e) {
			console.warn('[GitStore] Failed to discard file:', e);
		}
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
		await Promise.all([
			this.fetchGitInfo(projectPath),
			this.fetchWorktrees(projectPath),
			this.fetchStatus(projectPath),
			this.fetchLog(projectPath),
			this.fetchStashes(projectPath)
		]);
	}

	async refreshAll(projectPaths: string[]) {
		await Promise.all(projectPaths.map((p) => this.refreshGitState(p)));
	}
}
