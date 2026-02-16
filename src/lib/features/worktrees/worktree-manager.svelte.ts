import { ConfirmAction } from '$lib/utils/confirm-action.svelte';
import type { GitStore } from '$stores/git.svelte';
import type { ProjectStore } from '$stores/projects.svelte';
import type { WorkbenchSettingsStore } from '$stores/workbench-settings.svelte';
import type { WorkspaceStore } from '$stores/workspaces.svelte';
import type { BranchInfo, WorktreeCopyOptions } from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';

interface WorktreeRemoval {
	projectPath: string;
	worktreePath: string;
}

export class WorktreeManagerStore {
	dialogOpen = $state(false);
	dialogProjectPath = $state('');
	dialogBranches: BranchInfo[] = $state([]);
	dialogError = $state('');
	readonly removal = new ConfirmAction<WorktreeRemoval>();

	_pendingTaskLink: { cardId: string; boardId: string } | null = $state(null);
	_suggestedBranch = $state('');

	private projectStore: ProjectStore;
	private workspaceStore: WorkspaceStore;
	private gitStore: GitStore;
	private workbenchSettings: WorkbenchSettingsStore;

	constructor(
		projectStore: ProjectStore,
		workspaceStore: WorkspaceStore,
		gitStore: GitStore,
		workbenchSettings: WorkbenchSettingsStore
	) {
		this.projectStore = projectStore;
		this.workspaceStore = workspaceStore;
		this.gitStore = gitStore;
		this.workbenchSettings = workbenchSettings;
	}

	async add(projectPath: string) {
		this.dialogProjectPath = projectPath;
		this.dialogError = '';
		this.dialogBranches = [];
		this._pendingTaskLink = null;
		this._suggestedBranch = '';
		this.dialogOpen = true;

		try {
			this.dialogBranches = await invoke<BranchInfo[]>('list_branches', { path: projectPath });
		} catch (e) {
			this.dialogError = `Failed to list branches: ${String(e)}`;
		}
	}

	async addWithBranch(
		projectPath: string,
		suggestedBranch: string,
		cardId?: string,
		boardId?: string
	) {
		this.dialogProjectPath = projectPath;
		this.dialogError = '';
		this.dialogBranches = [];
		this.dialogOpen = true;

		this._pendingTaskLink = cardId && boardId ? { cardId, boardId } : null;
		this._suggestedBranch = suggestedBranch;

		try {
			this.dialogBranches = await invoke<BranchInfo[]>('list_branches', { path: projectPath });
		} catch (e) {
			this.dialogError = `Failed to list branches: ${String(e)}`;
		}
	}

	async create(branch: string, newBranch: boolean, path: string, copyOptions: WorktreeCopyOptions) {
		try {
			const createdPath = await invoke<string>('create_worktree', {
				request: {
					repoPath: this.dialogProjectPath,
					branch,
					newBranch,
					path,
					copyOptions,
					strategy: this.workbenchSettings.worktreeStrategy
				}
			});
			this.dialogOpen = false;
			await this.gitStore.refreshGitState(this.dialogProjectPath);

			const project = this.projectStore.getByPath(this.dialogProjectPath);
			if (project) {
				this.workspaceStore.openWorktree(project, createdPath, branch);
			}

			if (this._pendingTaskLink) {
				const { getTrelloStore } = await import('$stores/context');
				const trelloStore = getTrelloStore();
				trelloStore.linkTaskToBranch(
					this.dialogProjectPath,
					this._pendingTaskLink.cardId,
					this._pendingTaskLink.boardId,
					branch,
					createdPath
				);
				this._pendingTaskLink = null;
			}
		} catch (e) {
			this.dialogError = String(e);
		}
	}

	open(projectPath: string, worktreePath: string, branch: string) {
		const project = this.projectStore.getByPath(projectPath);
		if (!project) return;
		this.workspaceStore.openWorktree(project, worktreePath, branch);
	}

	remove(projectPath: string, worktreePath: string) {
		this.removal.request({ projectPath, worktreePath });
	}

	async confirmRemove(force = false) {
		await this.removal.confirm(async ({ projectPath, worktreePath }) => {
			await invoke('remove_worktree', { repoPath: projectPath, worktreePath, force });
			const ws = this.workspaceStore.getByWorktreePath(worktreePath);
			if (ws) this.workspaceStore.close(ws.id);
			await this.gitStore.refreshGitState(projectPath);
		});
	}
}
