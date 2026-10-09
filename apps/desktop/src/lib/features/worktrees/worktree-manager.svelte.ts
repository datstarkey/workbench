import { ConfirmAction } from '$lib/utils/confirm-action.svelte';
import type { GitHubStore } from '$stores/github.svelte';
import type { GitStore } from '$stores/git.svelte';
import type { ProjectStore } from '$stores/projects.svelte';
import type { WorkspaceStore } from '$stores/workspaces.svelte';
import type { BranchInfo, WorktreeCopyOptions } from '$types/workbench';
import { invoke } from '$lib/transport';

interface WorktreeRemoval {
	projectPath: string;
	worktreePath: string;
	branch: string;
	branchHasMergedPr: boolean;
}

export class WorktreeManagerStore {
	creating = $state(false);
	#dialogOpen = $state(false);
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

	/** Closing the dialog is ignored while a worktree is being created. */
	get dialogOpen() {
		return this.#dialogOpen;
	}

	set dialogOpen(value: boolean) {
		if (!this.creating) this.#dialogOpen = value;
	}

	constructor(
		projectStore: ProjectStore,
		workspaceStore: WorkspaceStore,
		gitStore: GitStore,
		githubStore: GitHubStore
	) {
		this.projectStore = projectStore;
		this.workspaceStore = workspaceStore;
		this.gitStore = gitStore;
		this.githubStore = githubStore;
	}

	async add(
		projectPath: string,
		options?: { suggestedBranch?: string; cardId?: string; boardId?: string }
	) {
		if (this.creating) return;
		this.dialogProjectPath = projectPath;
		this.dialogError = '';
		this.dialogBranches = [];
		this._pendingTaskLink =
			options?.cardId && options?.boardId
				? { cardId: options.cardId, boardId: options.boardId }
				: null;
		this._suggestedBranch = options?.suggestedBranch ?? '';
		this.dialogOpen = true;

		try {
			this.dialogBranches = await invoke<BranchInfo[]>('list_branches', { path: projectPath });
		} catch (e) {
			this.dialogError = `Failed to list branches: ${String(e)}`;
		}
	}

	/** The host fills the layout, start point and fetch from its settings. */
	async create(branch: string, newBranch: boolean, copyOptions: WorktreeCopyOptions) {
		if (this.creating) return;
		this.creating = true;
		this.dialogError = '';
		try {
			const createdPath = await invoke<string>('create_worktree', {
				request: { repoPath: this.dialogProjectPath, branch, newBranch, copyOptions }
			});
			this.#dialogOpen = false;
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
		} finally {
			this.creating = false;
		}
	}

	open(projectPath: string, worktreePath: string, branch: string) {
		const project = this.projectStore.getByPath(projectPath);
		if (!project) return;
		this.workspaceStore.openWorktree(project, worktreePath, branch);
	}

	remove(projectPath: string, worktreePath: string, branch: string) {
		if (this.removal.busy) return;
		const prs = this.githubStore.prsByProject[projectPath] ?? [];
		const branchHasMergedPr = prs.some((p) => p.headRefName === branch && p.state === 'MERGED');
		this.deleteBranchOnRemove = branchHasMergedPr;
		this.removal.request({ projectPath, worktreePath, branch, branchHasMergedPr });
	}

	async confirmRemove(force = false) {
		const deleteBranch = this.deleteBranchOnRemove;
		await this.removal.confirm(async ({ projectPath, worktreePath, branch }) => {
			const removeBranch = deleteBranch && !!branch;
			const removed = await invoke<{ branchDeleted?: boolean } | null>('remove_worktree', {
				repoPath: projectPath,
				worktreePath,
				force,
				deleteBranch: removeBranch
			});
			const ws = this.workspaceStore.getByWorktreePath(worktreePath);
			if (ws) this.workspaceStore.close(ws.id);
			// A host that predates `deleteBranch` leaves the branch to us.
			if (removeBranch && removed?.branchDeleted === undefined) {
				await invoke('delete_branch', { repoPath: projectPath, branch, force: false }).catch((e) =>
					console.warn('[WorktreeManager] Failed to delete branch:', e)
				);
			}
			await this.gitStore.refreshGitState(projectPath);
		});
	}
}
