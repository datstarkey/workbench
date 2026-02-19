import { ConfirmAction } from '$lib/utils/confirm-action.svelte';
import type { GitHubStore } from '$stores/github.svelte';
import type { GitStore } from '$stores/git.svelte';
import type { ProjectStore } from '$stores/projects.svelte';
import type { WorkbenchSettingsStore } from '$stores/workbench-settings.svelte';
import type { WorkspaceStore } from '$stores/workspaces.svelte';
import type { BranchInfo, WorktreeCopyOptions } from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';

interface WorktreeRemoval {
	projectPath: string;
	worktreePath: string;
	branch: string;
	branchHasMergedPr: boolean;
}

export class WorktreeManagerStore {
	dialogOpen = $state(false);
	dialogProjectPath = $state('');
	dialogBranches: BranchInfo[] = $state([]);
	dialogError = $state('');
	readonly removal = new ConfirmAction<WorktreeRemoval>();
	deleteBranchOnRemove = $state(false);

	_pendingTaskLink: { cardId: string; boardId: string } | null = $state(null);
	_suggestedBranch = $state('');

	private projectStore: ProjectStore;
	private workspaceStore: WorkspaceStore;
	private gitStore: GitStore;
	private githubStore: GitHubStore;
	private workbenchSettings: WorkbenchSettingsStore;

	constructor(
		projectStore: ProjectStore,
		workspaceStore: WorkspaceStore,
		gitStore: GitStore,
		githubStore: GitHubStore,
		workbenchSettings: WorkbenchSettingsStore
	) {
		this.projectStore = projectStore;
		this.workspaceStore = workspaceStore;
		this.gitStore = gitStore;
		this.githubStore = githubStore;
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
					strategy: this.workbenchSettings.worktreeStrategy,
					startPoint: newBranch ? this.workbenchSettings.worktreeStartPoint : undefined,
					fetchBeforeCreate: newBranch
						? this.workbenchSettings.worktreeFetchBeforeCreate
						: undefined
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

	remove(projectPath: string, worktreePath: string, branch: string) {
		const prs = this.githubStore.prsByProject[projectPath] ?? [];
		const branchHasMergedPr = prs.some((p) => p.headRefName === branch && p.state === 'MERGED');
		this.deleteBranchOnRemove = branchHasMergedPr;
		this.removal.request({ projectPath, worktreePath, branch, branchHasMergedPr });
	}

	async confirmRemove(force = false) {
		const deleteBranch = this.deleteBranchOnRemove;
		await this.removal.confirm(async ({ projectPath, worktreePath, branch }) => {
			await invoke('remove_worktree', { repoPath: projectPath, worktreePath, force });
			const ws = this.workspaceStore.getByWorktreePath(worktreePath);
			if (ws) this.workspaceStore.close(ws.id);
			if (deleteBranch && branch) {
				try {
					await invoke('delete_branch', { repoPath: projectPath, branch, force: false });
				} catch (e) {
					console.warn('[WorktreeManager] Failed to delete branch:', e);
				}
			}
			await this.gitStore.refreshGitState(projectPath);
		});
	}
}
