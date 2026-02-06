import type {
	ProjectConfig,
	ProjectWorkspace,
	SplitDirection,
	TerminalTabState
} from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';
import { claudeNewSessionCommand, claudeResumeCommand } from '$lib/claude';

interface WorkspaceSnapshot {
	workspaces: ProjectWorkspace[];
	selectedId: string | null;
}

const uid = (): string => {
	if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
		return crypto.randomUUID();
	}
	return `id-${Date.now()}-${Math.random().toString(16).slice(2)}`;
};

class WorkspaceStore {
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

	get activeTerminalTab(): TerminalTabState | null {
		const ws = this.activeWorkspace;
		if (!ws) return null;
		return (
			ws.terminalTabs.find((t) => t.id === ws.activeTerminalTabId) ?? ws.terminalTabs[0] ?? null
		);
	}

	getByProjectPath(projectPath: string): ProjectWorkspace | undefined {
		return this.workspaces.find((w) => w.projectPath === projectPath);
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
		invoke('save_workspaces', { snapshot }).catch(() => {});
	}

	async load() {
		try {
			const snapshot = await invoke<WorkspaceSnapshot>('load_workspaces');
			if (snapshot.workspaces.length > 0) {
				this.workspaces = snapshot.workspaces;
				this.selectedId = snapshot.selectedId;
			}
		} catch {
			// No saved workspaces — that's fine
		}
	}

	open(project: ProjectConfig): { claudeSessionId: string; tabLabel: string } | null {
		const existing = this.getByProjectPath(project.path);
		if (existing) {
			this.selectedId = existing.id;
			this.persist();
			return null;
		}

		const claudeSessionId = uid();
		const tabLabel = 'Claude 1';
		const initialTab = this.createClaudeTab(
			tabLabel,
			claudeSessionId,
			claudeNewSessionCommand(claudeSessionId)
		);
		const workspace: ProjectWorkspace = {
			id: uid(),
			projectPath: project.path,
			projectName: project.name,
			terminalTabs: [initialTab],
			activeTerminalTabId: initialTab.id
		};

		this.workspaces = [...this.workspaces, workspace];
		this.selectedId = workspace.id;
		this.persist();
		return { claudeSessionId, tabLabel };
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
		this.workspaces = this.workspaces.map((w) => {
			if (w.id !== workspaceId) return w;
			const newTab = this.createTerminalTab(project, false, w.terminalTabs.length + 1);
			return {
				...w,
				terminalTabs: [...w.terminalTabs, newTab],
				activeTerminalTabId: newTab.id
			};
		});
		this.persist();
	}

	closeTerminalTab(workspaceId: string, tabId: string) {
		this.workspaces = this.workspaces.map((w) => {
			if (w.id !== workspaceId || w.terminalTabs.length === 1) return w;
			const tabIndex = w.terminalTabs.findIndex((t) => t.id === tabId);
			const updatedTabs = w.terminalTabs.filter((t) => t.id !== tabId);
			const fallback = updatedTabs[tabIndex] || updatedTabs[tabIndex - 1] || updatedTabs[0];
			return {
				...w,
				terminalTabs: updatedTabs,
				activeTerminalTabId: w.activeTerminalTabId === tabId ? fallback.id : w.activeTerminalTabId
			};
		});
		this.persist();
	}

	setActiveTab(workspaceId: string, tabId: string) {
		this.workspaces = this.workspaces.map((w) => {
			if (w.id !== workspaceId) return w;
			return { ...w, activeTerminalTabId: tabId };
		});
		this.persist();
	}

	splitTerminal(workspaceId: string, direction: SplitDirection) {
		this.workspaces = this.workspaces.map((w) => {
			if (w.id !== workspaceId) return w;
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
		this.persist();
	}

	removePane(workspaceId: string, paneId: string) {
		this.workspaces = this.workspaces.map((w) => {
			if (w.id !== workspaceId) return w;
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
		this.persist();
	}

	addClaudeSession(workspaceId: string): { tabLabel: string; claudeSessionId: string } {
		const claudeSessionId = uid();
		let tabLabel = '';
		this.workspaces = this.workspaces.map((w) => {
			if (w.id !== workspaceId) return w;
			const claudeCount = w.terminalTabs.filter((t) => t.type === 'claude').length;
			tabLabel = `Claude ${claudeCount + 1}`;
			const newTab = this.createClaudeTab(
				tabLabel,
				claudeSessionId,
				claudeNewSessionCommand(claudeSessionId)
			);
			return {
				...w,
				terminalTabs: [...w.terminalTabs, newTab],
				activeTerminalTabId: newTab.id
			};
		});
		this.persist();
		return { tabLabel, claudeSessionId };
	}

	resumeClaudeSession(workspaceId: string, claudeSessionId: string, label: string) {
		this.workspaces = this.workspaces.map((w) => {
			if (w.id !== workspaceId) return w;
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
		this.persist();
	}

	ensureShape(getProject: (path: string) => ProjectConfig | undefined) {
		let changed = false;
		const normalized = this.workspaces.map((w) => {
			let nextTabs = w.terminalTabs;
			const project = getProject(w.projectPath);

			if (!Array.isArray(nextTabs) || nextTabs.length === 0) {
				if (project) {
					nextTabs = [this.createTerminalTab(project, false, 1)];
					changed = true;
				} else {
					nextTabs = [];
				}
			}

			nextTabs = nextTabs.map((tab) => {
				if (!Array.isArray(tab.panes) || tab.panes.length === 0) {
					changed = true;
					return { ...tab, panes: [{ id: uid() }] };
				}
				// Auto-resume: ensure Claude panes have resume command on app restart
				const fixedPanes = tab.panes.map((pane) => {
					if (pane.type === 'claude' && pane.claudeSessionId) {
						const resumeCmd = claudeResumeCommand(pane.claudeSessionId);
						if (pane.startupCommand !== resumeCmd) {
							changed = true;
							return { ...pane, startupCommand: resumeCmd };
						}
					}
					return pane;
				});
				return { ...tab, panes: fixedPanes };
			});

			const hasActiveTab = nextTabs.some((t) => t.id === w.activeTerminalTabId);
			const nextActiveId = hasActiveTab ? w.activeTerminalTabId : nextTabs[0]?.id || '';
			if (nextActiveId !== w.activeTerminalTabId) changed = true;

			if (nextTabs !== w.terminalTabs || nextActiveId !== w.activeTerminalTabId) {
				return { ...w, terminalTabs: nextTabs, activeTerminalTabId: nextActiveId };
			}
			return w;
		});

		if (changed) {
			this.workspaces = normalized;
		}
	}
}

export const workspaceStore = new WorkspaceStore();
