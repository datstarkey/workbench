import {
	isAISessionType,
	type AgentSummary,
	type PaneView,
	type ProjectConfig,
	type ProjectTask,
	type ProjectWorkspace,
	type SessionType,
	type SplitDirection,
	type TerminalPaneState,
	type TerminalTabState
} from '$types/workbench';
import { invoke } from '$lib/transport';
import {
	extractPromptArg,
	newSessionCommand,
	resumeCommand,
	tryResumeCommand,
	claudeNewSessionWithIdCommand,
	applyClaudeLaunchOptions,
	type LaunchOptions
} from '$lib/utils/claude';
import { effectivePath } from '$lib/utils/path';
import { getGitStore, getWorkbenchSettingsStore } from './context';
import { uid } from '$lib/utils/uid';
import { suppressLayout } from '$features/terminal/layout-guard';
import { visibleSplit } from '$features/terminal/split-view';
import { deleteServerTerminal } from '$features/terminal/terminal-connection';
import { stopAgent, stopAgentForPane } from '$features/chat/agent-api';
import { paneAgent, terminalAfterChat } from '$features/chat/pane-handoff';
import {
	chatHasHistory,
	isChatClaimed,
	releaseChat,
	reopenChat
} from '$features/chat/chat-registry';
import { paneDisplayName, type AdoptableTerminal } from '$features/terminal/server-terminals';
import { PaneAdoption, type AdoptedTab } from '$features/terminal/pane-adoption';

function moveById<T extends { id: string }>(items: T[], fromId: string, toId: string): T[] | null {
	const from = items.findIndex((i) => i.id === fromId);
	const to = items.findIndex((i) => i.id === toId);
	if (from === -1 || to === -1 || from === to) return null;
	const next = [...items];
	next.splice(to, 0, ...next.splice(from, 1));
	return next;
}

interface WorkspaceSnapshot {
	workspaces: ProjectWorkspace[];
	selectedId: string | null;
	/** Maps pane.id → server terminal ID for reload-survival re-attach. */
	serverTerminalIds?: Record<string, string>;
}

interface AddAISessionOptions {
	label?: string;
	startupCommand?: string;
}

export class WorkspaceStore {
	workspaces: ProjectWorkspace[] = $state([]);
	private _selectedId: string | null = $state(null);

	/**
	 * Maps pane.id → server-assigned terminal ID. Persisted in the workspace
	 * snapshot so panes can re-attach to surviving server PTYs after a reload.
	 */
	private serverTerminalIds: Record<string, string> = $state({});

	/**
	 * Panes adopted from another device's terminal or chat. Terminals mount
	 * detached (offering "Take control") instead of kicking that device, chats
	 * attach only; closing one only detaches, and they're left out of the
	 * persisted snapshot (a loopback PTY dies with the app).
	 */
	private adoption = new PaneAdoption();

	private settingsStore = getWorkbenchSettingsStore();
	private gitStore = getGitStore();

	private switchCallbacks: Array<(projectPath: string) => void> = [];

	private get launchOptions(): LaunchOptions {
		return this.settingsStore.launchOptions;
	}

	get selectedId(): string | null {
		return this._selectedId;
	}

	set selectedId(id: string | null) {
		const prevProjectPath = this.activeProjectPath;
		this._selectedId = id;
		const newProjectPath = this.activeProjectPath;
		if (newProjectPath && newProjectPath !== prevProjectPath) {
			for (const cb of this.switchCallbacks) {
				cb(newProjectPath);
			}
		}
	}

	onWorkspaceSwitch(callback: (projectPath: string) => void): void {
		this.switchCallbacks.push(callback);
	}

	get activeWorkspaceId(): string | null {
		if (this.selectedId && this.workspaces.some((w) => w.id === this.selectedId)) {
			return this.selectedId;
		}
		return this.workspaces[0]?.id ?? null;
	}

	get activeWorkspace(): ProjectWorkspace | null {
		return this.workspaces.find((w) => w.id === this.activeWorkspaceId) ?? null;
	}

	isProjectOpen(projectPath: string): boolean {
		return this.workspaces.some((w) => w.projectPath === projectPath);
	}

	get activeProjectPath(): string | null {
		return this.activeWorkspace?.projectPath ?? null;
	}

	/** Git status of the active workspace's cwd (worktree or main checkout) */
	readonly activeGitStatus = $derived.by(() => {
		const ws = this.activeWorkspace;
		return ws ? this.gitStore.statusByProject[effectivePath(ws)] : undefined;
	});

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

	private createTaskTerminalTab(task: ProjectTask, claudeAccountId?: string): TerminalTabState {
		return {
			id: uid(),
			label: task.name,
			split: 'horizontal',
			panes: [
				{ id: uid(), startupCommand: task.command, ...(claudeAccountId && { claudeAccountId }) }
			]
		};
	}

	private createAITab(
		label: string,
		sessionId: string,
		command: string,
		type: SessionType,
		claudeAccountId?: string
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
					startupCommand: command,
					...(type === 'claude' && claudeAccountId && { claudeAccountId })
				}
			]
		};
	}

	private persist() {
		const snapshot: WorkspaceSnapshot = {
			workspaces: this.adoption.persistable(this.workspaces),
			selectedId: this.selectedId,
			serverTerminalIds: Object.fromEntries(
				Object.entries(this.serverTerminalIds).filter(
					([paneId]) => !this.adoption.isAdopted(paneId)
				)
			)
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
				this.serverTerminalIds = snapshot.serverTerminalIds ?? {};
			}
		} catch (e) {
			console.warn('[WorkspaceStore] No saved workspaces:', e);
		}
	}

	/**
	 * Return the persisted server terminal ID for a pane, if any.
	 * TerminalPane uses this to attempt re-attach before creating a new PTY.
	 */
	getServerTerminalId(paneId: string): string | undefined {
		return this.serverTerminalIds[paneId];
	}

	/**
	 * Called by TerminalPane once the server PTY is assigned (create or re-attach).
	 * Persists the mapping so a webview reload can re-attach to the same PTY.
	 */
	setServerTerminalId(paneId: string, serverTerminalId: string): void {
		if (this.serverTerminalIds[paneId] === serverTerminalId) return;
		this.serverTerminalIds = { ...this.serverTerminalIds, [paneId]: serverTerminalId };
		this.persist();
	}

	/** Server terminal ids the adoption poller must skip: mapped to panes or released. */
	knownServerTerminalIds(): string[] {
		return this.adoption.knownTerminalIds(this.serverTerminalIds);
	}

	/** Shows another device's terminal (mounts detached) or chat (attaches only). */
	isAdoptedPane(paneId: string): boolean {
		return this.adoption.isAdopted(paneId);
	}

	/** An adopted chat that ended was restarted here: the pane now owns its session. */
	takeOverPane(paneId: string): void {
		if (this.adoption.takeOver(paneId)) this.persist();
	}

	/** The pane's chat was ended elsewhere (End on the phone): close it here too. */
	closeEndedChat(paneId: string): void {
		const at = this.findPaneLocation(paneId);
		if (!at) return;
		const tab = this.workspaces
			.find((w) => w.id === at.workspaceId)
			?.terminalTabs.find((t) => t.id === at.tabId);
		if (tab && tab.panes.length > 1) this.removePane(at.workspaceId, paneId);
		else this.closeTerminalTab(at.workspaceId, at.tabId);
	}

	/** Readable server-side name for a pane, e.g. `app [feat] · Claude 1`. */
	paneDisplayName(paneId: string): string | undefined {
		return paneDisplayName(this.workspaces, paneId);
	}

	/**
	 * Add a background tab for a terminal opened on another device, attached to
	 * its existing PTY. Returns false when no open workspace runs in its cwd.
	 */
	adoptServerTerminal(terminal: AdoptableTerminal): boolean {
		const adopted = this.adoption.terminalTab(this.workspaces, terminal);
		if (!adopted) return false;
		const paneId = adopted.tab.panes[0].id;
		this.serverTerminalIds = { ...this.serverTerminalIds, [paneId]: terminal.id };
		this.addBackgroundTab(adopted);
		return true;
	}

	/**
	 * The listed chats to adopt. Panes still on an id a `/clear` replaced follow
	 * it first, so the re-keyed session isn't mistaken for a new one.
	 */
	adoptableServerChats(list: AgentSummary[]): AgentSummary[] {
		for (const { paneId, sessionId, type } of this.adoption.rekeys(this.workspaces, list)) {
			this.updateAISessionByPaneId(paneId, sessionId, type);
		}
		return this.adoption.adoptableChats(this.workspaces, list, isChatClaimed);
	}

	/**
	 * Add a background chat tab for a session started on another device. Its
	 * chat attaches only, never starting a process of its own. Returns false
	 * when no open workspace runs in its cwd.
	 */
	adoptServerChat(chat: AgentSummary): boolean {
		const adopted = this.adoption.chatTab(this.workspaces, chat);
		if (adopted) this.addBackgroundTab(adopted);
		return adopted !== null;
	}

	private addBackgroundTab({ workspaceId, tab }: AdoptedTab): void {
		this.updateWorkspace(workspaceId, (w) => ({
			...w,
			terminalTabs: [...w.terminalTabs, tab],
			activeTerminalTabId: w.activeTerminalTabId || tab.id
		}));
	}

	private panesOf(ws: ProjectWorkspace): TerminalPaneState[] {
		return ws.terminalTabs.flatMap((t) => t.panes);
	}

	/**
	 * Kill the server-side PTYs (and any chat-mode agent process) for panes
	 * being intentionally closed (vs a webview reload, which only detaches). Without this the PTYs leak on the server and
	 * count against the terminal cap. Best-effort / fire-and-forget; also drops the
	 * persisted re-attach mappings so a stale id is never reused. Adopted panes
	 * belong to another device, so closing one only detaches and releases it.
	 */
	private disposeServerTerminals(panes: Iterable<TerminalPaneState>): void {
		const next = { ...this.serverTerminalIds };
		let changed = false;
		for (const pane of panes) {
			releaseChat(pane.id);
			const serverId = next[pane.id];
			const owned = this.adoption.release(pane, serverId);
			if (owned) void stopAgentForPane(pane.id);
			if (serverId) {
				if (owned) void deleteServerTerminal(serverId);
				delete next[pane.id];
				changed = true;
			}
		}
		if (changed) {
			this.serverTerminalIds = next;
			this.persist();
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
			renderer: this.settingsStore.terminalRenderer,
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
		const closing = this.getWorkspacesForProject(projectPath);
		const ids = closing.map((w) => w.id);
		if (ids.length === 0) return;

		this.disposeServerTerminals(closing.flatMap((w) => this.panesOf(w)));
		this.workspaces = this.workspaces.filter((w) => !ids.includes(w.id));

		if (this.selectedId && ids.includes(this.selectedId)) {
			this.selectedId = this.workspaces[0]?.id ?? null;
		}
		this.persist();
	}

	close(workspaceId: string) {
		const ws = this.workspaces.find((w) => w.id === workspaceId);
		if (!ws) return;

		this.disposeServerTerminals(this.panesOf(ws));
		const idx = this.workspaces.indexOf(ws);
		this.workspaces = this.workspaces.filter((w) => w.id !== workspaceId);

		if (this.selectedId === workspaceId) {
			const fallback = this.workspaces[idx] || this.workspaces[idx - 1] || null;
			this.selectedId = fallback?.id ?? null;
		}
		this.persist();
	}

	reorder(fromId: string, toId: string) {
		const next = moveById(this.workspaces, fromId, toId);
		if (!next) return;
		this.workspaces = next;
		this.persist();
	}

	reorderTerminalTab(workspaceId: string, fromId: string, toId: string) {
		const ws = this.workspaces.find((w) => w.id === workspaceId);
		const tabs = ws && moveById(ws.terminalTabs, fromId, toId);
		if (tabs) this.updateWorkspace(workspaceId, (w) => ({ ...w, terminalTabs: tabs }));
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

	addProjectTaskTab(
		workspaceId: string,
		task: ProjectTask,
		claudeAccountId?: string
	): { tabId: string } {
		const tab = this.createTaskTerminalTab(task, claudeAccountId);
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
		const tab = this.workspaces
			.find((w) => w.id === workspaceId)
			?.terminalTabs.find((t) => t.id === tabId);
		if (tab) this.disposeServerTerminals(tab.panes);
		this.updateWorkspace(workspaceId, (w) => {
			const tabIndex = w.terminalTabs.findIndex((t) => t.id === tabId);
			const updatedTabs = w.terminalTabs.filter((t) => t.id !== tabId);
			const fallback = updatedTabs[tabIndex] || updatedTabs[tabIndex - 1] || updatedTabs[0];
			return {
				...w,
				terminalTabs: updatedTabs,
				activeTerminalTabId:
					w.activeTerminalTabId === tabId ? (fallback?.id ?? '') : w.activeTerminalTabId,
				splitView: w.splitView?.tabIds.includes(tabId) ? undefined : w.splitView
			};
		});
	}

	setActiveTab(workspaceId: string, tabId: string) {
		this.updateWorkspace(workspaceId, (w) => ({ ...w, activeTerminalTabId: tabId }));
	}

	/**
	 * Show the active tab beside its neighbour (a new shell tab when it's the
	 * only one). Re-pressing the current direction unsplits.
	 */
	splitTerminal(workspaceId: string, direction: SplitDirection, project: ProjectConfig) {
		this.updateWorkspace(workspaceId, (w) => {
			const index = w.terminalTabs.findIndex((t) => t.id === w.activeTerminalTabId);
			if (index === -1 || w.renderer === 'native') return w;
			// Judge by what's on screen: a stale splitView (a tab replaced or dropped) is replaced.
			const shown = visibleSplit(w);
			if (shown) {
				return {
					...w,
					splitView:
						shown.direction === direction
							? undefined
							: { direction, tabIds: [shown.tabs[0].id, shown.tabs[1].id] }
				};
			}
			let tabs = w.terminalTabs;
			let partner = tabs[index + 1] ?? tabs[index - 1];
			if (!partner) {
				partner = this.createTerminalTab(project, false, tabs.length + 1);
				tabs = [...tabs, partner];
			}
			return {
				...w,
				terminalTabs: tabs,
				splitView: { direction, tabIds: [w.activeTerminalTabId, partner.id] }
			};
		});
	}

	removePane(workspaceId: string, paneId: string) {
		let removed: TerminalPaneState | undefined;
		suppressLayout(() => {
			this.updateWorkspace(workspaceId, (w) => {
				// A split shows two tabs, so the pane's tab may not be the active one.
				const tab = w.terminalTabs.find((t) => t.panes.some((p) => p.id === paneId));
				// Guard keeps the last pane.
				if (!tab || tab.panes.length <= 1) return w;
				removed = tab.panes.find((p) => p.id === paneId);
				const updatedTab: TerminalTabState = {
					...tab,
					panes: tab.panes.filter((p) => p.id !== paneId)
				};
				return {
					...w,
					terminalTabs: w.terminalTabs.map((t) => (t.id === tab.id ? updatedTab : t))
				};
			});
		});
		// Kill the removed pane's server PTY so it doesn't leak.
		if (removed) this.disposeServerTerminals([removed]);
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
			const explicit = options?.startupCommand?.trim();
			// A project/task startup command of `claude …` must still pick up the
			// sandbox wrapper and permission mode, or both settings are bypassed.
			const startupCommand = explicit
				? type === 'codex'
					? explicit
					: applyClaudeLaunchOptions(explicit, this.launchOptions)
				: newSessionCommand(type, this.launchOptions);
			// A plain new Claude tab can open straight into chat: chat picks the
			// session id up front (`--session-id`), so it needs no terminal first.
			// Not while the sandbox runtime is on — chat can't run inside it yet.
			const asChat = type === 'claude' && !explicit && this.opensAsChat;
			const sessionId = asChat ? crypto.randomUUID() : ''; // Claude requires a real UUID
			const newTab = this.createAITab(
				label,
				sessionId,
				asChat ? claudeNewSessionWithIdCommand(sessionId, this.launchOptions) : startupCommand,
				type,
				this.settingsStore.activeClaudeAccountId
			);
			if (asChat) newTab.panes[0].view = 'chat';
			tabId = newTab.id;
			return {
				...w,
				terminalTabs: [...w.terminalTabs, newTab],
				activeTerminalTabId: newTab.id
			};
		});
		return { tabId };
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
					const cmd = tryResumeCommand(type, sessionId, this.launchOptions);
					return {
						...p,
						claudeSessionId: sessionId,
						...(cmd && { startupCommand: cmd })
					};
				});
				return tabChanged ? { ...t, panes } : t;
			})
		}));
		if (changed) this.persist();
	}

	/** Find workspace/tab containing any pane (AI or otherwise). */
	findPaneLocation(paneId: string): { workspaceId: string; tabId: string } | null {
		for (const ws of this.workspaces) {
			for (const tab of ws.terminalTabs) {
				if (tab.panes.some((p) => p.id === paneId)) {
					return { workspaceId: ws.id, tabId: tab.id };
				}
			}
		}
		return null;
	}

	/**
	 * Move an AI pane's session between its terminal and chat. Only one process
	 * may own a session, so the current one stops first: the PTY (and the TUI in
	 * it) is killed before chat resumes the session, and the chat process is
	 * stopped before the terminal reopens it. A Codex pane with no session yet
	 * starts a new thread in chat, whose id the chat hands back to the pane.
	 */
	async setPaneView(paneId: string, view: PaneView): Promise<void> {
		const pane = this.workspaces
			.flatMap((w) => w.terminalTabs.flatMap((t) => t.panes))
			.find((p) => p.id === paneId);
		if (!pane || (pane.view ?? 'terminal') === view) return;
		// Claude chat can't run inside the sandbox runtime (which never wraps Codex):
		// refuse before killing the terminal.
		if (view === 'chat' && paneAgent(pane) === 'claude' && this.settingsStore.sandboxRuntimeEnabled)
			return;
		this.adoption.takeOver(paneId);
		let patch: Partial<TerminalPaneState> = { view };
		if (view === 'chat') {
			const serverId = this.serverTerminalIds[paneId];
			if (serverId) {
				const rest = { ...this.serverTerminalIds };
				delete rest[paneId];
				this.serverTerminalIds = rest;
				await deleteServerTerminal(serverId);
			}
		} else {
			// A chat that never got a message has no session file to resume yet.
			const started = chatHasHistory(paneId);
			releaseChat(paneId);
			await (
				pane.claudeSessionId ? stopAgent(pane.claudeSessionId) : stopAgentForPane(paneId)
			).catch(() => {});
			patch = { ...terminalAfterChat(pane, started, this.launchOptions), view };
		}
		this.patchPane(paneId, patch);
	}

	private patchPane(paneId: string, patch: Partial<TerminalPaneState>): void {
		const location = this.findPaneLocation(paneId);
		if (!location) return;
		this.updateWorkspace(location.workspaceId, (w) => ({
			...w,
			terminalTabs: w.terminalTabs.map((t) =>
				t.id !== location.tabId
					? t
					: { ...t, panes: t.panes.map((p) => (p.id === paneId ? { ...p, ...patch } : p)) }
			)
		}));
	}

	/** New Claude tabs open as chat: the setting, and never inside the sandbox runtime. */
	private get opensAsChat(): boolean {
		return (
			this.settingsStore.defaultClaudeView === 'chat' && !this.settingsStore.sandboxRuntimeEnabled
		);
	}

	/**
	 * `/resume` in a chat: stop the pane's current conversation and continue
	 * another one in its place. The grid re-keys the chat on the new id.
	 */
	async resumeInChat(paneId: string, sessionId: string, label: string): Promise<void> {
		const pane = this.workspaces
			.flatMap((w) => w.terminalTabs.flatMap((t) => t.panes))
			.find((p) => p.id === paneId);
		if (!pane || pane.claudeSessionId === sessionId) return;
		this.adoption.takeOver(paneId);
		releaseChat(paneId);
		if (pane.claudeSessionId) await stopAgent(pane.claudeSessionId).catch(() => {});
		const type = paneAgent(pane);
		this.updateAISessionByPaneId(paneId, sessionId, type);
		this.updateAITabLabelByPaneId(paneId, label, type);
	}

	/** The pane shows its Claude or Codex session as chat (see `setPaneView`). */
	isChatPane(paneId: string): boolean {
		return this.workspaces.some((w) =>
			w.terminalTabs.some((t) => t.panes.some((p) => p.id === paneId && p.view === 'chat'))
		);
	}

	/** Activate the workspace and tab containing the given pane. */
	focusPane(paneId: string): boolean {
		const location = this.findPaneLocation(paneId);
		if (!location) return false;
		this.selectedId = location.workspaceId;
		this.setActiveTab(location.workspaceId, location.tabId);
		return true;
	}

	/** Find workspace/tab context for an AI pane. */
	/**
	 * `cwd` is the worktree-aware path the pane actually runs in — session JSONL is
	 * keyed by that, not by the parent project, so a worktree tab would otherwise look
	 * up the wrong encoded directory.
	 */
	findAIPaneContext(
		paneId: string,
		type: SessionType = 'claude'
	): { workspaceId: string; tabId: string; projectPath: string; cwd: string } | null {
		for (const ws of this.workspaces) {
			for (const tab of ws.terminalTabs) {
				if (tab.type !== type) continue;
				if (tab.panes.some((p) => p.id === paneId)) {
					return {
						workspaceId: ws.id,
						tabId: tab.id,
						projectPath: ws.projectPath,
						cwd: effectivePath(ws)
					};
				}
			}
		}
		return null;
	}

	/** Update an AI tab label by pane ID. */
	updateAITabLabelByPaneId(paneId: string, label: string, type: SessionType = 'claude') {
		for (const ws of this.workspaces) {
			for (const tab of ws.terminalTabs) {
				if (tab.type !== type) continue;
				if (!tab.panes.some((p) => p.id === paneId)) continue;
				if (tab.label === label) return;
				this.updateWorkspace(ws.id, (w) => ({
					...w,
					terminalTabs: w.terminalTabs.map((t) => (t.id === tab.id ? { ...t, label } : t))
				}));
				return;
			}
		}
	}

	async restartAISession(workspaceId: string, tabId: string): Promise<void> {
		// Restart mints a fresh tab with new pane ids, so the old tab's server PTYs
		// would leak (no close path runs for them). Kill them first.
		const oldTab = this.workspaces
			.find((w) => w.id === workspaceId)
			?.terminalTabs.find((t) => t.id === tabId);
		// Another device's chat keeps running: re-attach (an ended one restarts here).
		const adopted = oldTab?.panes.filter((p) => this.adoption.isAdopted(p.id)) ?? [];
		if (adopted.length > 0) {
			for (const pane of adopted) reopenChat(pane.id);
			return;
		}
		// An empty Codex thread may never have been written: restart a fresh one.
		const first = oldTab?.panes[0];
		const freshCodex =
			first?.type === 'codex' && first.view === 'chat' && !chatHasHistory(first.id);
		// A chat pane's agent must be gone before the new pane starts the same
		// session, or two processes own it. Terminal tabs skip this and stay sync.
		for (const pane of oldTab?.panes ?? []) {
			if (pane.view !== 'chat' || !pane.claudeSessionId) continue;
			releaseChat(pane.id);
			await stopAgent(pane.claudeSessionId).catch(() => {});
		}
		if (oldTab && isAISessionType(oldTab.type)) {
			this.disposeServerTerminals(oldTab.panes);
		}
		this.updateWorkspace(workspaceId, (w) => {
			const tab = w.terminalTabs.find((t) => t.id === tabId);
			if (!tab || !isAISessionType(tab.type)) return w;
			const type = tab.type;
			const sessionId = freshCodex ? undefined : tab.panes[0]?.claudeSessionId;
			const command = sessionId
				? resumeCommand(type, sessionId, this.launchOptions)
				: newSessionCommand(type, this.launchOptions);
			// A restart stays on the pane's account: its transcript lives there.
			const newTab = this.createAITab(
				tab.label,
				sessionId ?? '',
				command,
				type,
				tab.panes[0]?.claudeAccountId
			);
			// Codex chat needs no id (a new thread) and never runs inside the sandbox runtime.
			const chatAllowed =
				type === 'codex' || (sessionId && !this.settingsStore.sandboxRuntimeEnabled);
			if (tab.panes[0]?.view === 'chat' && chatAllowed) newTab.panes[0].view = 'chat';
			const splitView = w.splitView && {
				...w.splitView,
				tabIds: w.splitView.tabIds.map((id) => (id === tabId ? newTab.id : id)) as [string, string]
			};
			return {
				...w,
				terminalTabs: w.terminalTabs.map((t) => (t.id === tabId ? newTab : t)),
				activeTerminalTabId: newTab.id,
				splitView
			};
		});
	}

	/** `accountId` is the account owning the transcript; a session only resumes there. */
	resumeAISession(
		workspaceId: string,
		sessionId: string,
		label: string,
		type: SessionType = 'claude',
		accountId?: string
	) {
		this.updateWorkspace(workspaceId, (w) => {
			const newTab = this.createAITab(
				label,
				sessionId,
				resumeCommand(type, sessionId, this.launchOptions),
				type,
				accountId
			);
			if (type === 'claude' && this.opensAsChat) newTab.panes[0].view = 'chat';
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

	async restartClaudeByProject(projectPath: string, tabId: string): Promise<void> {
		await this.withWorkspaceForProjectTab(projectPath, tabId, (ws) =>
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

	/** `claudeAccountId` runs the task's shell under that Claude login (e.g. `claude auth login`). */
	runTaskInWorkspace(
		workspaceId: string,
		task: ProjectTask,
		claudeAccountId?: string
	): { workspaceId: string; tabId: string } {
		this.selectedId = workspaceId;
		const { tabId } = this.addProjectTaskTab(workspaceId, task, claudeAccountId);
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
				const isAI = isAISessionType(pane.type);
				if (isAI && pane.claudeSessionId) {
					const cmd = resumeCommand(pane.type!, pane.claudeSessionId, this.launchOptions);
					if (pane.startupCommand !== cmd) {
						changed = true;
						return { ...pane, startupCommand: cmd };
					}
				} else if (isAI && !pane.claudeSessionId) {
					// Rebuild from the binary alone, so a changed permission mode adds or drops
					// the flag while any initial prompt argument (e.g. `claude 'review ...'`)
					// survives. Unrecognised commands normalise back to the bare base.
					const base = newSessionCommand(pane.type!, this.launchOptions);
					const promptArg = extractPromptArg(pane.type!, pane.startupCommand);
					const cmd = promptArg ? `${base} ${promptArg}` : base;
					if (pane.startupCommand !== cmd) {
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
		return this.gitStore.branchByProject[ws.projectPath] ?? ws.branch;
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
