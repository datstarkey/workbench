import type {
	GitHubBranchRuns,
	GitHubBranchStatus,
	GitHubCheckDetail,
	GitHubCheckTransitionEvent,
	GitHubPR,
	GitHubProjectStatus,
	GitHubProjectStatusEvent,
	ProjectRefreshRequestedEvent,
	GitHubRemote
} from '$types/workbench';
import { getClaudeSessionStore, getGitStore, getProjectStore, getWorkspaceStore } from './context';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { load } from '@tauri-apps/plugin-store';

type CheckNotification = {
	name: string;
	bucket: 'pass' | 'fail';
	projectPath: string;
	prNumber: number;
};

export class GitHubStore {
	private workspaces = getWorkspaceStore();
	private git = getGitStore();
	private sessions = getClaudeSessionStore();
	private projects = getProjectStore();

	ghAvailable: boolean | null = $state(null);
	remoteByProject: Record<string, GitHubRemote | null> = $state({});
	prsByProject: Record<string, GitHubPR[]> = $state({});
	branchRunsByProject: Record<string, Record<string, GitHubBranchRuns>> = $state({});

	// Sidebar state
	sidebarOpen: boolean = $state(false);
	private _overrideTarget: { projectPath: string; branch: string } | null = $state(null);
	private _overrideForWorkspaceId: string | null = $state(null);
	checksByPr: Record<string, GitHubCheckDetail[]> = $state({});

	/** Derived sidebar target: override (valid only for current workspace) or active workspace */
	readonly sidebarTarget = $derived.by(() => {
		if (this._overrideTarget && this._overrideForWorkspaceId === this.workspaces.activeWorkspaceId)
			return this._overrideTarget;
		const ws = this.workspaces.activeWorkspace;
		if (!ws) return null;
		const branch = this.workspaces.resolvedBranch(ws) ?? null;
		if (!branch) return null;
		return { projectPath: ws.projectPath, branch };
	});

	/** PR matching the current sidebar target branch */
	readonly sidebarPr = $derived.by((): GitHubPR | null => {
		const target = this.sidebarTarget;
		if (!target) return null;
		const prs = this.prsByProject[target.projectPath] ?? [];
		return prs.find((p) => p.headRefName === target.branch) ?? null;
	});

	/** Branch workflow runs for the current sidebar target */
	readonly sidebarBranchRuns = $derived.by((): GitHubBranchRuns | null => {
		const target = this.sidebarTarget;
		if (!target) return null;
		return this.branchRunsByProject[target.projectPath]?.[target.branch] ?? null;
	});

	/** PR checks for the current sidebar PR */
	readonly sidebarChecks = $derived.by((): GitHubCheckDetail[] => {
		const target = this.sidebarTarget;
		const pr = this.sidebarPr;
		if (!target || !pr) return [];
		return this.checksByPr[this.prKey(target.projectPath, pr.number)] ?? [];
	});

	/** Derived list of branches with active AI sessions */
	readonly activeBranches = $derived.by(() => {
		if (this.ghAvailable === false) return [];
		const sessionsByProject = this.sessions.activeSessionsByProject;
		const branches: Array<{ projectPath: string; branch: string }> = [];
		for (const project of this.projects.projects) {
			const hasSessions = (sessionsByProject[project.path]?.length ?? 0) > 0;
			if (!hasSessions) continue;
			const branch = this.git.branchByProject[project.path];
			if (branch) {
				branches.push({ projectPath: project.path, branch });
			}
			const worktrees = (this.git.worktreesByProject[project.path] ?? []).filter(
				(wt) => !wt.isMain
			);
			for (const wt of worktrees) {
				if (wt.branch) {
					branches.push({ projectPath: project.path, branch: wt.branch });
				}
			}
		}
		return branches;
	});

	readonly trackedProjectPaths = $derived.by(() => {
		// eslint-disable-next-line svelte/prefer-svelte-reactivity -- plain utility set for dedupe
		const seen = new Set<string>();
		for (const { projectPath } of this.activeBranches) {
			seen.add(projectPath);
		}
		return Array.from(seen);
	});

	// Notification callback — registered by consumer (App.svelte), called directly on transitions
	private onCheckCompleteCallback: ((notification: CheckNotification) => void) | null = null;

	// Not reactive — internal bookkeeping only
	// eslint-disable-next-line svelte/prefer-svelte-reactivity
	private pendingProjects = new Set<string>();
	private trackedProjectsFingerprint = '';

	constructor() {
		listen<ProjectRefreshRequestedEvent>('project:refresh-requested', (event) => {
			void this.refreshProject(event.payload.projectPath);
		});
		listen<GitHubProjectStatusEvent>('github:project-status', (event) => {
			this.applyProjectStatus(event.payload.projectPath, event.payload.status);
		});
		listen<GitHubCheckTransitionEvent>('github:check-transition', (event) => {
			const callback = this.onCheckCompleteCallback;
			if (!callback) return;
			callback({
				name: event.payload.name,
				bucket: event.payload.bucket,
				projectPath: event.payload.projectPath,
				prNumber: event.payload.prNumber
			});
		});
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
			this.applyProjectStatus(projectPath, status);
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
		const branchRuns = this.branchRunsByProject[projectPath]?.[branch] ?? null;
		return { pr, remote, branchRuns };
	}

	getRemoteUrl(projectPath: string): string | null {
		return this.remoteByProject[projectPath]?.htmlUrl ?? null;
	}

	async refreshProject(projectPath: string): Promise<void> {
		if (this.ghAvailable !== true) return;
		try {
			await invoke('github_refresh_project', { projectPath });
		} catch (e) {
			console.warn('[GitHubStore] Failed to request project refresh:', e);
		}
	}

	private applyProjectStatus(projectPath: string, status: GitHubProjectStatus): void {
		if (this.ghAvailable === false) return;
		this.remoteByProject = { ...this.remoteByProject, [projectPath]: status.remote };
		this.prsByProject = { ...this.prsByProject, [projectPath]: status.prs };
		this.branchRunsByProject = {
			...this.branchRunsByProject,
			[projectPath]: status.branchRuns
		};

		const prefix = `${projectPath}::`;
		const nextChecksByPr = { ...this.checksByPr };
		// eslint-disable-next-line svelte/prefer-svelte-reactivity -- plain utility set for key tracking
		const incomingKeys = new Set<string>();

		for (const [prNum, checks] of Object.entries(status.prChecks)) {
			const prNumber = Number(prNum);
			if (!Number.isFinite(prNumber)) continue;
			const key = this.prKey(projectPath, prNumber);
			incomingKeys.add(key);
			nextChecksByPr[key] = checks;
		}

		for (const key of Object.keys(nextChecksByPr)) {
			if (key.startsWith(prefix) && !incomingKeys.has(key)) {
				delete nextChecksByPr[key];
			}
		}

		this.checksByPr = nextChecksByPr;
	}

	async syncTrackedProjects(): Promise<void> {
		if (this.ghAvailable !== true) return;
		const projectPaths = this.trackedProjectPaths.slice().sort();
		const fingerprint = projectPaths.join('\n');
		if (fingerprint === this.trackedProjectsFingerprint) return;
		this.trackedProjectsFingerprint = fingerprint;

		try {
			await invoke('github_set_tracked_projects', { projectPaths });
		} catch (e) {
			console.warn('[GitHubStore] Failed to sync tracked projects:', e);
		}
	}

	// --- PR checks / sidebar ---

	private prKey(projectPath: string, prNumber: number): string {
		return `${projectPath}::${prNumber}`;
	}

	onCheckComplete(callback: (notification: CheckNotification) => void): void {
		this.onCheckCompleteCallback = callback;
	}

	getPrChecks(projectPath: string, prNumber: number): GitHubCheckDetail[] | undefined {
		return this.checksByPr[this.prKey(projectPath, prNumber)];
	}

	showBranch(projectPath: string, branch: string): void {
		this._overrideTarget = { projectPath, branch };
		this._overrideForWorkspaceId = this.workspaces.activeWorkspaceId;
		void this.refreshProject(projectPath);
	}

	clearSidebarOverride(): void {
		this._overrideTarget = null;
		this._overrideForWorkspaceId = null;
	}

	async toggleSidebar(): Promise<void> {
		this.sidebarOpen = !this.sidebarOpen;
		await this.persistSidebarState();
	}

	async initSidebarState(): Promise<void> {
		try {
			const store = await load('ui-state.json');
			const open = await store.get<boolean>('githubSidebarOpen');
			if (typeof open === 'boolean') {
				this.sidebarOpen = open;
			}
		} catch {
			// Ignore — first run, no persisted state
		}
	}

	private async persistSidebarState(): Promise<void> {
		try {
			const store = await load('ui-state.json');
			await store.set('githubSidebarOpen', this.sidebarOpen);
			await store.save();
		} catch {
			// Non-critical
		}
	}

	/** Call after projects load — checks gh availability, then fetches status per project */
	async initForProjects(projectPaths: string[]): Promise<void> {
		const available = await this.checkGhAvailable();
		if (!available) return;

		await invoke('github_set_tracked_projects', { projectPaths });
		this.trackedProjectsFingerprint = projectPaths.slice().sort().join('\n');
		await Promise.all(projectPaths.map((p) => this.refreshProject(p)));
	}

	destroy() {}
}
