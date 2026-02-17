import type {
	ProjectConfig,
	ProjectTask,
	ProjectWorkspace,
	SessionType,
	SplitDirection,
	TerminalTabState
} from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';
import { newSessionCommand, resumeCommand } from '$lib/utils/claude';
import { getGitStore } from './context';
import { uid } from '$lib/utils/uid';

interface WorkspaceSnapshot {
	workspaces: ProjectWorkspace[];
	selectedId: string | null;
}

interface AddAISessionOptions {
	label?: string;
	startupCommand?: string;
}

export class WorkspaceStore {
	workspaces: ProjectWorkspace[] = $state([]);
	selectedId: string | null = $state(null);

	get activeWorkspaceId(): string | null {
		if (this.selectedId && this.workspaces.some((w) => w.id === this.selectedId)) {
			return this.selectedId;
		}
		return this.workspaces[0]?.id ?? null;
	}

	get activeWorkspace(): ProjectWorkspace | null {
		return this.workspaces.find((w) => w.id === this.activeWorkspaceId) ?? null;
	}

	get openProjectPaths(): string[] {
		return this.workspaces.map((w) => w.projectPath);
	}

	get activeProjectPath(): string | null {
		return this.activeWorkspace?.projectPath ?? null;
	}

	get activeTerminalTab(): TerminalTabState | null {
		const ws = this.activeWorkspace;
		if (!ws) return null;
		return (
			ws.terminalTabs.find((t) => t.id === ws.activeTerminalTabId) ?? ws.terminalTabs[0] ?? null
		);
	}

	/** Find the main workspace (not a worktree) for a project path */
	getByProjectPath(projectPath: string): ProjectWorkspace | undefined {
		return this.workspaces.find((w) => w.projectPath === projectPath && !w.worktreePath);
	}

	/** Find a worktree workspace by its worktree path */
	getByWorktreePath(worktreePath: string): ProjectWorkspace | undefined {
		return this.workspaces.find((w) => w.worktreePath === worktreePath);
	}

	/** Get all workspaces (main + worktrees) for a project */
	getWorkspacesForProject(projectPath: string): ProjectWorkspace[] {
		return this.workspaces.filter((w) => w.projectPath === projectPath);
	}

	private createTerminalTab(
		project: ProjectConfig,
		firstForProject: boolean,
		tabIndex: number
	): TerminalTabState {
		return {
			id: uid(),
			label: `Terminal ${tabIndex}`,
			split: 'horizontal',
			panes: [
				{
					id: uid(),
					startupCommand: firstForProject ? project.startupCommand : undefined
				}
			]
		};
	}

	private createTaskTerminalTab(task: ProjectTask): TerminalTabState {
		return {
			id: uid(),
			label: task.name,
			split: 'horizontal',
			panes: [{ id: uid(), startupCommand: task.command }]
		};
	}

	private createAITab(
		label: string,
		sessionId: string,
		command: string,
		type: SessionType
	): TerminalTabState {
		return {
			id: uid(),
			label,
			split: 'horizontal',
			type,
			panes: [
				{
					id: uid(),
					type,
					claudeSessionId: sessionId,
					startupCommand: command
				}
			]
		};
	}

	private persist() {
		const snapshot: WorkspaceSnapshot = {
			workspaces: this.workspaces,
			selectedId: this.selectedId
		};
		invoke('save_workspaces', { snapshot }).catch((e) => {
			console.error('[WorkspaceStore] Failed to persist:', e);
		});
	}

	/** Apply an updater function to a single workspace by ID, then persist. */
	private updateWorkspace(
		workspaceId: string,
		updater: (ws: ProjectWorkspace) => ProjectWorkspace
	): void {
		this.workspaces = this.workspaces.map((w) => (w.id === workspaceId ? updater(w) : w));
		this.persist();
	}

	async load() {
		try {
			const snapshot = await invoke<WorkspaceSnapshot>('load_workspaces');
			if (snapshot.workspaces.length > 0) {
				this.workspaces = snapshot.workspaces;
				this.selectedId = snapshot.selectedId;
			}
		} catch (e) {
			console.warn('[WorkspaceStore] No saved workspaces:', e);
		}
	}

	private openInternal(project: ProjectConfig, opts?: { worktreePath: string; branch: string }) {
		const existing = opts
			? this.getByWorktreePath(opts.worktreePath)
			: this.getByProjectPath(project.path);
		if (existing) {
			this.selectedId = existing.id;
			this.persist();
			return;
		}

		const isNewProject = !this.workspaces.some((w) => w.projectPath === project.path);

		const workspace: ProjectWorkspace = {
			id: uid(),
			projectPath: project.path,
			projectName: project.name,
			terminalTabs: [],
			activeTerminalTabId: '',
			...opts
		};

		this.workspaces = [...this.workspaces, workspace];
		this.selectedId = workspace.id;
		this.persist();

		if (isNewProject) getGitStore().watchProject(project.path);
	}

	open(project: ProjectConfig) {
		this.openInternal(project);
	}

	openWorktree(project: ProjectConfig, worktreePath: string, branch: string) {
		this.openInternal(project, { worktreePath, branch });
	}

	closeAllForProject(projectPath: string) {
		const ids = this.getWorkspacesForProject(projectPath).map((w) => w.id);
		if (ids.length === 0) return;

		this.workspaces = this.workspaces.filter((w) => !ids.includes(w.id));

		if (this.selectedId && ids.includes(this.selectedId)) {
			this.selectedId = this.workspaces[0]?.id ?? null;
		}
		this.persist();
		getGitStore().unwatchProject(projectPath);
	}

	close(workspaceId: string) {
		const ws = this.workspaces.find((w) => w.id === workspaceId);
		if (!ws) return;
		const projectPath = ws.projectPath;

		const idx = this.workspaces.indexOf(ws);
		this.workspaces = this.workspaces.filter((w) => w.id !== workspaceId);

		if (this.selectedId === workspaceId) {
			const fallback = this.workspaces[idx] || this.workspaces[idx - 1] || null;
			this.selectedId = fallback?.id ?? null;
		}
		this.persist();

		if (!this.workspaces.some((w) => w.projectPath === projectPath)) {
			getGitStore().unwatchProject(projectPath);
		}
	}

	reorder(fromId: string, toId: string) {
		const fromIndex = this.workspaces.findIndex((w) => w.id === fromId);
		const toIndex = this.workspaces.findIndex((w) => w.id === toId);
		if (fromIndex === -1 || toIndex === -1 || fromIndex === toIndex) return;

		const next = [...this.workspaces];
		const [moved] = next.splice(fromIndex, 1);
		next.splice(toIndex, 0, moved);
		this.workspaces = next;
		this.persist();
	}

	updateProjectInfo(previousPath: string, newPath: string, newName: string) {
		this.workspaces = this.workspaces.map((w) => {
			if (w.projectPath !== previousPath) return w;
			return { ...w, projectPath: newPath, projectName: newName };
		});
		this.persist();
	}

	addTerminalTab(workspaceId: string, project: ProjectConfig) {
		this.updateWorkspace(workspaceId, (w) => {
			const newTab = this.createTerminalTab(project, false, w.terminalTabs.length + 1);
			return {
				...w,
				terminalTabs: [...w.terminalTabs, newTab],
				activeTerminalTabId: newTab.id
			};
		});
	}

	addProjectTaskTab(workspaceId: string, task: ProjectTask): { tabId: string } {
		const tab = this.createTaskTerminalTab(task);
		this.updateWorkspace(workspaceId, (w) => {
			return {
				...w,
				terminalTabs: [...w.terminalTabs, tab],
				activeTerminalTabId: tab.id
			};
		});
		return { tabId: tab.id };
	}

	closeTerminalTab(workspaceId: string, tabId: string) {
		this.updateWorkspace(workspaceId, (w) => {
			const tabIndex = w.terminalTabs.findIndex((t) => t.id === tabId);
			const updatedTabs = w.terminalTabs.filter((t) => t.id !== tabId);
			const fallback = updatedTabs[tabIndex] || updatedTabs[tabIndex - 1] || updatedTabs[0];
			return {
				...w,
				terminalTabs: updatedTabs,
				activeTerminalTabId:
					w.activeTerminalTabId === tabId ? (fallback?.id ?? '') : w.activeTerminalTabId
			};
		});
	}

	setActiveTab(workspaceId: string, tabId: string) {
		this.updateWorkspace(workspaceId, (w) => ({ ...w, activeTerminalTabId: tabId }));
	}

	splitTerminal(workspaceId: string, direction: SplitDirection) {
		this.updateWorkspace(workspaceId, (w) => {
			const tab = w.terminalTabs.find((t) => t.id === w.activeTerminalTabId);
			if (!tab) return w;
			const updatedTab: TerminalTabState = {
				...tab,
				split: direction,
				panes: [...tab.panes, { id: uid() }]
			};
			return {
				...w,
				terminalTabs: w.terminalTabs.map((t) => (t.id === tab.id ? updatedTab : t))
			};
		});
	}

	removePane(workspaceId: string, paneId: string) {
		this.updateWorkspace(workspaceId, (w) => {
			const tab = w.terminalTabs.find((t) => t.id === w.activeTerminalTabId);
			if (!tab || tab.panes.length <= 1) return w;
			const updatedTab: TerminalTabState = {
				...tab,
				panes: tab.panes.filter((p) => p.id !== paneId)
			};
			return {
				...w,
				terminalTabs: w.terminalTabs.map((t) => (t.id === tab.id ? updatedTab : t))
			};
		});
	}

	/** Add a new AI session tab (Claude or Codex) */
	addAISession(
		workspaceId: string,
		type: SessionType = 'claude',
		options?: AddAISessionOptions
	): { tabId: string } {
		let tabId = '';
		const labelPrefix = type === 'codex' ? 'Codex' : 'Claude';
		this.updateWorkspace(workspaceId, (w) => {
			const count = w.terminalTabs.filter((t) => t.type === type).length;
			const label = options?.label?.trim() || `${labelPrefix} ${count + 1}`;
			const startupCommand = options?.startupCommand?.trim() || newSessionCommand(type);
			const newTab = this.createAITab(label, '', startupCommand, type);
			tabId = newTab.id;
			return {
				...w,
				terminalTabs: [...w.terminalTabs, newTab],
				activeTerminalTabId: newTab.id
			};
		});
		return { tabId };
	}

	/** Backward compat wrapper */
	addClaudeSession(workspaceId: string): { tabId: string } {
		return this.addAISession(workspaceId, 'claude');
	}

	/** Update an AI tab once its session ID has been discovered from the JSONL */
	updateAITab(
		workspaceId: string,
		tabId: string,
		sessionId: string,
		label: string,
		type: SessionType = 'claude'
	) {
		this.updateWorkspace(workspaceId, (w) => ({
			...w,
			terminalTabs: w.terminalTabs.map((t) => {
				if (t.id !== tabId) return t;
				return {
					...t,
					label,
					panes: t.panes.map((p) => (p.type === type ? { ...p, claudeSessionId: sessionId } : p))
				};
			})
		}));
	}

	/** Update an AI pane's session ID by pane ID. */
	updateAISessionByPaneId(paneId: string, sessionId: string, type: SessionType = 'claude') {
		let changed = false;
		this.workspaces = this.workspaces.map((w) => ({
			...w,
			terminalTabs: w.terminalTabs.map((t) => {
				if (t.type !== type) return t;
				let tabChanged = false;
				const panes = t.panes.map((p) => {
					if (p.id !== paneId || p.claudeSessionId === sessionId) return p;
					tabChanged = true;
					changed = true;
					return { ...p, claudeSessionId: sessionId };
				});
				return tabChanged ? { ...t, panes } : t;
			})
		}));
		if (changed) this.persist();
	}

	/** Find workspace/tab context for an AI pane. */
	findAIPaneContext(
		paneId: string,
		type: SessionType = 'claude'
	): { workspaceId: string; tabId: string; projectPath: string } | null {
		for (const ws of this.workspaces) {
			for (const tab of ws.terminalTabs) {
				if (tab.type !== type) continue;
				if (tab.panes.some((p) => p.id === paneId)) {
					return { workspaceId: ws.id, tabId: tab.id, projectPath: ws.projectPath };
				}
			}
		}
		return null;
	}

	/** Update an AI tab label by pane ID. */
	updateAITabLabelByPaneId(paneId: string, label: string, type: SessionType = 'claude') {
		const ctx = this.findAIPaneContext(paneId, type);
		if (!ctx) return;

		const ws = this.workspaces.find((w) => w.id === ctx.workspaceId);
		const tab = ws?.terminalTabs.find((t) => t.id === ctx.tabId);
		if (!tab || tab.label === label) return;

		this.updateWorkspace(ctx.workspaceId, (w) => ({
			...w,
			terminalTabs: w.terminalTabs.map((t) => (t.id === ctx.tabId ? { ...t, label } : t))
		}));
	}

	/** Backward compat wrapper */
	updateClaudeTab(workspaceId: string, tabId: string, sessionId: string, label: string) {
		this.updateAITab(workspaceId, tabId, sessionId, label, 'claude');
	}

	restartAISession(workspaceId: string, tabId: string) {
		this.updateWorkspace(workspaceId, (w) => {
			const tab = w.terminalTabs.find((t) => t.id === tabId);
			if (!tab || (tab.type !== 'claude' && tab.type !== 'codex')) return w;
			const type = tab.type;
			const sessionId = tab.panes[0]?.claudeSessionId;
			const command = sessionId ? resumeCommand(type, sessionId) : newSessionCommand(type);
			const newTab = this.createAITab(tab.label, sessionId ?? '', command, type);
			return {
				...w,
				terminalTabs: w.terminalTabs.map((t) => (t.id === tabId ? newTab : t)),
				activeTerminalTabId: newTab.id
			};
		});
	}

	/** Backward compat wrapper */
	restartClaudeSession(workspaceId: string, tabId: string) {
		this.restartAISession(workspaceId, tabId);
	}

	resumeAISession(
		workspaceId: string,
		sessionId: string,
		label: string,
		type: SessionType = 'claude'
	) {
		this.updateWorkspace(workspaceId, (w) => {
			const newTab = this.createAITab(label, sessionId, resumeCommand(type, sessionId), type);
			return {
				...w,
				terminalTabs: [...w.terminalTabs, newTab],
				activeTerminalTabId: newTab.id
			};
		});
	}

	/** Backward compat wrapper */
	resumeClaudeSession(workspaceId: string, claudeSessionId: string, label: string) {
		this.resumeAISession(workspaceId, claudeSessionId, label, 'claude');
	}

	// --- projectPath-based convenience methods ---

	private withMainWorkspace<T>(
		projectPath: string,
		fn: (ws: ProjectWorkspace) => T
	): T | undefined {
		const ws = this.getByProjectPath(projectPath);
		if (!ws) return undefined;
		return fn(ws);
	}

	private withWorkspaceForProjectTab<T>(
		projectPath: string,
		tabId: string,
		fn: (ws: ProjectWorkspace) => T
	): T | undefined {
		const ws = this.workspaces.find(
			(w) => w.projectPath === projectPath && w.terminalTabs.some((t) => t.id === tabId)
		);
		if (ws) return fn(ws);
		return this.withMainWorkspace(projectPath, fn);
	}

	selectTabByProject(projectPath: string, tabId: string) {
		this.withWorkspaceForProjectTab(projectPath, tabId, (ws) => {
			this.selectedId = ws.id;
			this.setActiveTab(ws.id, tabId);
		});
	}

	restartClaudeByProject(projectPath: string, tabId: string) {
		this.withWorkspaceForProjectTab(projectPath, tabId, (ws) =>
			this.restartAISession(ws.id, tabId)
		);
	}

	closeTabByProject(projectPath: string, tabId: string) {
		this.withWorkspaceForProjectTab(projectPath, tabId, (ws) =>
			this.closeTerminalTab(ws.id, tabId)
		);
	}

	addClaudeByProject(projectPath: string): { workspaceId: string; tabId: string } | null {
		return this.addAIByProject(projectPath, 'claude');
	}

	addAIByProject(
		projectPath: string,
		type: SessionType = 'claude',
		options?: AddAISessionOptions
	): { workspaceId: string; tabId: string } | null {
		return (
			this.withMainWorkspace(projectPath, (ws) => {
				const { tabId } = this.addAISession(ws.id, type, options);
				return { workspaceId: ws.id, tabId };
			}) ?? null
		);
	}

	runTaskByProject(
		projectPath: string,
		task: ProjectTask
	): { workspaceId: string; tabId: string } | null {
		return (
			this.withMainWorkspace(projectPath, (ws) => {
				const { tabId } = this.addProjectTaskTab(ws.id, task);
				return { workspaceId: ws.id, tabId };
			}) ?? null
		);
	}

	runTaskInWorkspace(
		workspaceId: string,
		task: ProjectTask
	): { workspaceId: string; tabId: string } {
		this.selectedId = workspaceId;
		const { tabId } = this.addProjectTaskTab(workspaceId, task);
		return { workspaceId, tabId };
	}

	// --- ensureShape sub-methods ---

	private ensureTabStructure(tabs: TerminalTabState[]): {
		tabs: TerminalTabState[];
		changed: boolean;
	} {
		let changed = false;
		if (!Array.isArray(tabs)) return { tabs: [], changed: true };

		const fixed = tabs.map((tab) => {
			if (!Array.isArray(tab.panes) || tab.panes.length === 0) {
				changed = true;
				return { ...tab, panes: [{ id: uid() }] };
			}
			return tab;
		});
		return { tabs: fixed, changed };
	}

	private ensureAIResumeCommands(tabs: TerminalTabState[]): {
		tabs: TerminalTabState[];
		changed: boolean;
	} {
		let changed = false;
		const fixed = tabs.map((tab) => {
			const fixedPanes = tab.panes.map((pane) => {
				const isAI = pane.type === 'claude' || pane.type === 'codex';
				if (isAI && pane.claudeSessionId) {
					const cmd = resumeCommand(pane.type!, pane.claudeSessionId);
					if (pane.startupCommand !== cmd) {
						changed = true;
						return { ...pane, startupCommand: cmd };
					}
				} else if (isAI && !pane.claudeSessionId) {
					const cmd = newSessionCommand(pane.type!);
					// Preserve explicit initial prompt invocations (e.g. `claude 'review ...'`).
					// Safe because we control startupCommand values — they are always built via
					// newSessionCommand() or agentActionCommand(), so `startsWith(cmd + ' ')` is
					// a reliable check for whether an initial prompt arg is appended.
					const current = pane.startupCommand?.trim();
					const hasPromptArg = current?.startsWith(`${cmd} `) ?? false;
					if (pane.startupCommand !== cmd && !hasPromptArg) {
						changed = true;
						return { ...pane, startupCommand: cmd };
					}
				}
				return pane;
			});
			return { ...tab, panes: fixedPanes };
		});
		return { tabs: fixed, changed };
	}

	private ensureActiveTabId(
		tabs: TerminalTabState[],
		currentActiveId: string
	): { activeId: string; changed: boolean } {
		if (tabs.length === 0) {
			return { activeId: '', changed: currentActiveId !== '' };
		}
		const hasActiveTab = tabs.some((t) => t.id === currentActiveId);
		if (hasActiveTab) return { activeId: currentActiveId, changed: false };
		return { activeId: tabs[0]?.id || '', changed: true };
	}

	/** Resolve the current branch for a workspace. Worktrees use their fixed branch; main workspaces derive from git. */
	resolvedBranch(ws: ProjectWorkspace): string | undefined {
		if (ws.worktreePath) return ws.branch;
		return getGitStore().branchByProject[ws.projectPath] ?? ws.branch;
	}

	ensureShape() {
		let anyChanged = false;
		const normalized = this.workspaces.map((w) => {
			const structure = this.ensureTabStructure(w.terminalTabs);
			const commands = this.ensureAIResumeCommands(structure.tabs);
			const activeTab = this.ensureActiveTabId(commands.tabs, w.activeTerminalTabId);

			const changed = structure.changed || commands.changed || activeTab.changed;
			if (changed) anyChanged = true;

			if (!changed) return w;
			return {
				...w,
				terminalTabs: commands.tabs,
				activeTerminalTabId: activeTab.activeId
			};
		});

		if (anyChanged) {
			this.workspaces = normalized;
		}
	}
}
