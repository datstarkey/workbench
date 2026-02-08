import type {
	ProjectConfig,
	ProjectWorkspace,
	SplitDirection,
	TerminalTabState
} from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';
import { CLAUDE_NEW_SESSION_COMMAND, claudeResumeCommand } from '$lib/utils/claude';
import { uid } from '$lib/utils/uid';

interface WorkspaceSnapshot {
	workspaces: ProjectWorkspace[];
	selectedId: string | null;
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

	private createClaudeTab(label: string, sessionId: string, command: string): TerminalTabState {
		return {
			id: uid(),
			label,
			split: 'horizontal',
			type: 'claude',
			panes: [
				{
					id: uid(),
					type: 'claude',
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
	}

	close(workspaceId: string) {
		const idx = this.workspaces.findIndex((w) => w.id === workspaceId);
		if (idx === -1) return;

		this.workspaces = this.workspaces.filter((w) => w.id !== workspaceId);

		if (this.selectedId === workspaceId) {
			const fallback = this.workspaces[idx] || this.workspaces[idx - 1] || null;
			this.selectedId = fallback?.id ?? null;
		}
		this.persist();
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

	addClaudeSession(workspaceId: string): { tabId: string } {
		let tabId = '';
		this.updateWorkspace(workspaceId, (w) => {
			const claudeCount = w.terminalTabs.filter((t) => t.type === 'claude').length;
			const label = `Claude ${claudeCount + 1}`;
			const newTab = this.createClaudeTab(label, '', CLAUDE_NEW_SESSION_COMMAND);
			tabId = newTab.id;
			return {
				...w,
				terminalTabs: [...w.terminalTabs, newTab],
				activeTerminalTabId: newTab.id
			};
		});
		return { tabId };
	}

	/** Update a Claude tab once its session ID has been discovered from the JSONL */
	updateClaudeTab(workspaceId: string, tabId: string, sessionId: string, label: string) {
		this.updateWorkspace(workspaceId, (w) => ({
			...w,
			terminalTabs: w.terminalTabs.map((t) => {
				if (t.id !== tabId) return t;
				return {
					...t,
					label,
					panes: t.panes.map((p) =>
						p.type === 'claude' ? { ...p, claudeSessionId: sessionId } : p
					)
				};
			})
		}));
	}

	restartClaudeSession(workspaceId: string, tabId: string) {
		this.updateWorkspace(workspaceId, (w) => {
			const tab = w.terminalTabs.find((t) => t.id === tabId);
			if (!tab || tab.type !== 'claude') return w;
			const sessionId = tab.panes[0]?.claudeSessionId;
			const command = sessionId ? claudeResumeCommand(sessionId) : CLAUDE_NEW_SESSION_COMMAND;
			const newTab = this.createClaudeTab(tab.label, sessionId ?? '', command);
			return {
				...w,
				terminalTabs: w.terminalTabs.map((t) => (t.id === tabId ? newTab : t)),
				activeTerminalTabId: newTab.id
			};
		});
	}

	resumeClaudeSession(workspaceId: string, claudeSessionId: string, label: string) {
		this.updateWorkspace(workspaceId, (w) => {
			const newTab = this.createClaudeTab(
				label,
				claudeSessionId,
				claudeResumeCommand(claudeSessionId)
			);
			return {
				...w,
				terminalTabs: [...w.terminalTabs, newTab],
				activeTerminalTabId: newTab.id
			};
		});
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

	selectTabByProject(projectPath: string, tabId: string) {
		this.withMainWorkspace(projectPath, (ws) => {
			this.selectedId = ws.id;
			this.setActiveTab(ws.id, tabId);
		});
	}

	restartClaudeByProject(projectPath: string, tabId: string) {
		this.withMainWorkspace(projectPath, (ws) => this.restartClaudeSession(ws.id, tabId));
	}

	closeTabByProject(projectPath: string, tabId: string) {
		this.withMainWorkspace(projectPath, (ws) => this.closeTerminalTab(ws.id, tabId));
	}

	addClaudeByProject(projectPath: string): { workspaceId: string; tabId: string } | null {
		return (
			this.withMainWorkspace(projectPath, (ws) => {
				const { tabId } = this.addClaudeSession(ws.id);
				return { workspaceId: ws.id, tabId };
			}) ?? null
		);
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

	private ensureClaudeResumeCommands(tabs: TerminalTabState[]): {
		tabs: TerminalTabState[];
		changed: boolean;
	} {
		let changed = false;
		const fixed = tabs.map((tab) => {
			const fixedPanes = tab.panes.map((pane) => {
				if (pane.type === 'claude' && pane.claudeSessionId) {
					const resumeCmd = claudeResumeCommand(pane.claudeSessionId);
					if (pane.startupCommand !== resumeCmd) {
						changed = true;
						return { ...pane, startupCommand: resumeCmd };
					}
				} else if (pane.type === 'claude' && !pane.claudeSessionId) {
					const newCmd = CLAUDE_NEW_SESSION_COMMAND;
					if (pane.startupCommand !== newCmd) {
						changed = true;
						return { ...pane, startupCommand: newCmd };
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

	ensureShape() {
		let anyChanged = false;
		const normalized = this.workspaces.map((w) => {
			const structure = this.ensureTabStructure(w.terminalTabs);
			const commands = this.ensureClaudeResumeCommands(structure.tabs);
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
