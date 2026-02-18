import type { ClaudeHookEvent, GitChangedEvent, WorktreeInfo } from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export class GitStore {
	worktreesByProject: Record<string, WorktreeInfo[]> = $state({});
	branchByProject: Record<string, string> = $state({});

	// eslint-disable-next-line svelte/prefer-svelte-reactivity
	private debounceTimers = new Map<string, ReturnType<typeof setTimeout>>();

	constructor() {
		listen<GitChangedEvent>('git:changed', (event) => {
			this.refreshGitState(event.payload.projectPath);
		});

		listen<ClaudeHookEvent>('claude:hook', (event) => {
			if (event.payload.hookEventName !== 'PostToolUse') return;
			const payload = event.payload.hookPayload;
			if (payload['tool_name'] !== 'Bash') return;
			const cmd = (payload['tool_input'] as Record<string, unknown>)?.['command'];
			if (typeof cmd !== 'string' || !/\bgit\b|\bgh\b/.test(cmd)) return;
			const cwd = event.payload.cwd;
			if (!cwd) return;
			const projectPath = Object.keys(this.branchByProject).find(
				(p) => cwd === p || cwd.startsWith(p + '/')
			);
			if (projectPath) this.debouncedRefreshGitState(projectPath);
		});
	}

	private debouncedRefreshGitState(projectPath: string): void {
		const existing = this.debounceTimers.get(projectPath);
		if (existing) clearTimeout(existing);
		this.debounceTimers.set(
			projectPath,
			setTimeout(() => {
				this.debounceTimers.delete(projectPath);
				this.refreshGitState(projectPath);
			}, 500)
		);
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

	async watchProject(projectPath: string) {
		try {
			await invoke('watch_project', { path: projectPath });
		} catch (e) {
			console.warn('[GitStore] Failed to watch project:', e);
		}
	}

	async unwatchProject(projectPath: string) {
		try {
			await invoke('unwatch_project', { path: projectPath });
		} catch (e) {
			console.warn('[GitStore] Failed to unwatch project:', e);
		}
	}
}
