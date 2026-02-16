import type {
	BoardConfig,
	MergeAction,
	TaskLink,
	TrelloBoard,
	TrelloBoardData,
	TrelloCard,
	TrelloCredentials,
	TrelloProjectConfig
} from '$types/trello';
import type { GitHubPR } from '$types/workbench';
import { getWorkspaceStore, getGitStore, getGitHubStore } from './context';
import { invoke } from '@tauri-apps/api/core';

export class TrelloStore {
	// --- State ---
	credentials: TrelloCredentials | null = $state(null);
	authenticated = $state(false);
	configByProject: Record<string, TrelloProjectConfig> = $state({});
	boardDataCache: Record<string, TrelloBoardData> = $state({});
	availableBoards: TrelloBoard[] = $state([]);
	loading = $state(false);
	sidebarTab: 'github' | 'boards' = $state<'github' | 'boards'>('github');

	// Private refs to other stores
	private workspaces = getWorkspaceStore();
	private git = getGitStore();
	private github = getGitHubStore();

	// Track previous PR states for merge detection
	private prevPrStates: Record<string, string> = {};

	// --- Derived ---
	readonly activeProjectPath = $derived(this.workspaces.activeWorkspace?.projectPath ?? null);

	readonly activeBoards = $derived.by((): BoardConfig[] => {
		const pp = this.activeProjectPath;
		if (!pp) return [];
		return this.configByProject[pp]?.boards ?? [];
	});

	readonly activeTaskLinks = $derived.by((): TaskLink[] => {
		const pp = this.activeProjectPath;
		if (!pp) return [];
		return this.configByProject[pp]?.taskLinks ?? [];
	});

	readonly activeBranch = $derived.by((): string | null => {
		const ws = this.workspaces.activeWorkspace;
		if (!ws) return null;
		return ws.branch ?? this.git.branchByProject[ws.projectPath] ?? null;
	});

	readonly linkedTask = $derived.by((): TaskLink | null => {
		const branch = this.activeBranch;
		if (!branch) return null;
		return this.activeTaskLinks.find((t) => t.branch === branch) ?? null;
	});

	readonly activeBoardData = $derived.by((): TrelloBoardData[] => {
		const pp = this.activeProjectPath;
		if (!pp) return [];
		const boards = this.configByProject[pp]?.boards ?? [];
		return boards
			.map((b) => this.boardDataCache[`${pp}::${b.boardId}`])
			.filter((d): d is TrelloBoardData => d != null);
	});

	// --- Methods ---

	setSidebarTab(tab: 'github' | 'boards'): void {
		this.sidebarTab = tab;
	}

	async loadCredentials(): Promise<void> {
		try {
			const creds = await invoke<TrelloCredentials | null>('trello_load_credentials');
			this.credentials = creds;
			this.authenticated = creds != null;
		} catch (e) {
			console.warn('[TrelloStore] Failed to load credentials:', e);
		}
	}

	async saveCredentials(apiKey: string, token: string): Promise<void> {
		await invoke('trello_save_credentials', { apiKey, token });
		this.credentials = { apiKey, token };
		this.authenticated = true;
	}

	async validateAuth(apiKey: string, token: string): Promise<boolean> {
		return invoke<boolean>('trello_validate_auth', { apiKey, token });
	}

	async disconnect(): Promise<void> {
		await invoke('trello_disconnect');
		this.credentials = null;
		this.authenticated = false;
		this.availableBoards = [];
		this.boardDataCache = {};
	}

	async loadProjectConfig(projectPath: string): Promise<void> {
		try {
			const config = await invoke<TrelloProjectConfig>('trello_load_project_config', {
				projectPath
			});
			this.configByProject = { ...this.configByProject, [projectPath]: config };
		} catch (e) {
			console.warn('[TrelloStore] Failed to load project config:', e);
		}
	}

	async saveProjectConfig(projectPath: string, config: TrelloProjectConfig): Promise<void> {
		await invoke('trello_save_project_config', { projectPath, config });
		this.configByProject = { ...this.configByProject, [projectPath]: config };
	}

	async fetchAvailableBoards(): Promise<void> {
		if (!this.credentials) return;
		this.loading = true;
		try {
			this.availableBoards = await invoke<TrelloBoard[]>('trello_list_boards', {
				apiKey: this.credentials.apiKey,
				token: this.credentials.token
			});
		} catch (e) {
			console.warn('[TrelloStore] Failed to fetch boards:', e);
			this.availableBoards = [];
		} finally {
			this.loading = false;
		}
	}

	async fetchBoardData(projectPath: string, boardConfig: BoardConfig): Promise<void> {
		try {
			const data = await invoke<TrelloBoardData>('trello_fetch_board_data', {
				boardId: boardConfig.boardId,
				hiddenColumns: boardConfig.hiddenColumns
			});
			const key = `${projectPath}::${boardConfig.boardId}`;
			this.boardDataCache = { ...this.boardDataCache, [key]: data };
		} catch (e) {
			console.warn('[TrelloStore] Failed to fetch board data:', e);
		}
	}

	async refreshAllBoards(projectPath: string): Promise<void> {
		const boards = this.configByProject[projectPath]?.boards ?? [];
		await Promise.all(boards.map((b) => this.fetchBoardData(projectPath, b)));
	}

	async createCard(projectPath: string, listId: string, name: string): Promise<TrelloCard | null> {
		try {
			const card = await invoke<TrelloCard>('trello_create_card', { listId, name });
			await this.refreshAllBoards(projectPath);
			return card;
		} catch (e) {
			console.warn('[TrelloStore] Failed to create card:', e);
			return null;
		}
	}

	async moveCard(cardId: string, targetListId: string): Promise<void> {
		await invoke('trello_move_card', { cardId, targetListId });
	}

	async addLabel(cardId: string, labelId: string): Promise<void> {
		await invoke('trello_add_label', { cardId, labelId });
	}

	async removeLabel(cardId: string, labelId: string): Promise<void> {
		await invoke('trello_remove_label', { cardId, labelId });
	}

	linkTaskToBranch(
		projectPath: string,
		cardId: string,
		boardId: string,
		branch: string,
		worktreePath?: string
	): void {
		const config = this.configByProject[projectPath] ?? { boards: [], taskLinks: [] };
		const existing = config.taskLinks.findIndex((t) => t.cardId === cardId);
		const link: TaskLink = { cardId, boardId, branch, worktreePath, projectPath };
		const taskLinks =
			existing >= 0
				? config.taskLinks.map((t, i) => (i === existing ? link : t))
				: [...config.taskLinks, link];
		const updated = { ...config, taskLinks };
		this.saveProjectConfig(projectPath, updated);

		// Execute link action if configured
		const board = config.boards.find((b) => b.boardId === boardId);
		if (board?.linkAction) {
			this.executeBoardAction(cardId, board.linkAction, projectPath);
		}
	}

	unlinkTask(projectPath: string, cardId: string): void {
		const config = this.configByProject[projectPath];
		if (!config) return;
		const updated = {
			...config,
			taskLinks: config.taskLinks.filter((t) => t.cardId !== cardId)
		};
		this.saveProjectConfig(projectPath, updated);
	}

	/** Detect merged PRs and execute merge actions (move card, add labels) */
	checkForMergedPrs(prsByProject: Record<string, GitHubPR[]>): void {
		for (const [projectPath, prs] of Object.entries(prsByProject)) {
			const config = this.configByProject[projectPath];
			if (!config) continue;

			for (const pr of prs) {
				const key = `${projectPath}::${pr.headRefName}`;
				const prevState = this.prevPrStates[key];
				this.prevPrStates[key] = pr.state;

				// Detect OPEN -> MERGED transition
				if (prevState && prevState !== 'MERGED' && pr.state === 'MERGED') {
					const link = config.taskLinks.find((t) => t.branch === pr.headRefName);
					if (!link) continue;
					const board = config.boards.find((b) => b.boardId === link.boardId);
					if (!board?.mergeAction) continue;

					this.executeBoardAction(link.cardId, board.mergeAction, projectPath);
				}
			}
		}
	}

	private async executeBoardAction(
		cardId: string,
		action: MergeAction,
		projectPath: string
	): Promise<void> {
		try {
			if (action.moveToColumnId) {
				await this.moveCard(cardId, action.moveToColumnId);
			}
			for (const labelId of action.addLabelIds ?? []) {
				await this.addLabel(cardId, labelId);
			}
			for (const labelId of action.removeLabelIds ?? []) {
				await this.removeLabel(cardId, labelId);
			}
			await this.refreshAllBoards(projectPath);
		} catch (e) {
			console.error('[TrelloStore] Board action failed:', e);
		}
	}
}
