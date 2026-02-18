import type { ProjectRefreshRequestedEvent, WorktreeInfo } from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export class GitStore {
	worktreesByProject: Record<string, WorktreeInfo[]> = $state({});
	branchByProject: Record<string, string> = $state({});

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

	async refreshGitState(projectPath: string) {
		await Promise.all([this.fetchGitInfo(projectPath), this.fetchWorktrees(projectPath)]);
	}

	async refreshAll(projectPaths: string[]) {
		await Promise.all(projectPaths.map((p) => this.refreshGitState(p)));
	}
}
