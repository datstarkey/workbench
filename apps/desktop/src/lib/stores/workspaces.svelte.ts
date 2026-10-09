import { toast } from 'svelte-sonner';
import type { ControlPlaneTransport } from '@workbench/transport';
import type {
	PaneView,
	ProjectConfig,
	ProjectTask,
	ProjectWorkspace,
	SessionType,
	SplitDirection,
	TerminalPaneState,
	TerminalTabState
} from '$types/workbench';
import {
	paneNotices,
	type WorkspaceCommand,
	type WorkspaceCommandResult,
	type WorkspaceSnapshot
} from '$types/workspace';
import { transport } from '$lib/transport';
import { effectivePath } from '$lib/utils/path';
import { releaseChat } from '$features/chat/chat-registry';
import { getGitStore, getWorkbenchSettingsStore } from './context';
import { hasSavedUi, loadUi, pruneUi, saveUi, seededUi, type WorkspaceUi } from './workspace-ui';
import { workspaceView } from './workspace-view';

export type WorkspaceApi = Pick<ControlPlaneTransport, 'workspaceCommand' | 'subscribeWorkspace'>;

/** Boot doesn't wait longer than this for the server's first snapshot. */
const FIRST_SNAPSHOT_MS = 5000;

interface PaneAt {
	workspace: ProjectWorkspace;
	tab: TerminalTabState;
	pane: TerminalPaneState;
}

interface NewSessionOptions {
	label?: string;
	/** Submitted as the new session starts (an agent action). */
	prompt?: string;
}

/**
 * The server's workspace model as this window renders it. Workspaces, tabs and
 * panes come only from the server's snapshots (`/events/workspace`); every
 * change is a command the server applies, and shows in a later snapshot. The
 * store adds this device's own choices (`WorkspaceUi`): selection, active tabs
 * and the Claude Terminal/Chat view.
 */
export class WorkspaceStore {
	private snapshot = $state.raw<WorkspaceSnapshot>({ rev: 0, workspaces: [] });
	private ui = $state.raw<WorkspaceUi>(loadUi());
	/** No state saved on this device yet: take the one an older desktop saved with the model. */
	private seed = !hasSavedUi();
	/** Ids a command made, kept from pruning until a snapshot at that rev arrives. */
	private pending: Record<string, number> = {};
	/** Spawn notices already shown, by pane and spawn. */
	private shownNotices: Record<string, true> = {};
	private unsubscribe: (() => void) | null = null;
	private settingsStore = getWorkbenchSettingsStore();
	private gitStore = getGitStore();
	private switchCallbacks: Array<(projectPath: string) => void> = [];

	private readonly api: WorkspaceApi;

	constructor(api: WorkspaceApi = transport()) {
		this.api = api;
	}

	/** Whether the server saves the model; anything but `ok` loses tabs on quit. */
	readonly persistence = $derived(this.snapshot.persistence ?? { status: 'ok', message: null });

	/** Every workspace, tab and pane id the snapshot shows. */
	private readonly ids: Record<string, true> = $derived(
		Object.fromEntries(
			this.snapshot.workspaces.flatMap((w) => [
				[w.id, true],
				...w.tabs.flatMap((t) => [[t.id, true], ...t.panes.map((p) => [p.id, true])])
			])
		)
	);

	readonly workspaces: ProjectWorkspace[] = $derived.by(() =>
		this.snapshot.workspaces.map((w) => workspaceView(w, this.ui))
	);

	private readonly paneIndex: Record<string, PaneAt> = $derived(
		Object.fromEntries(
			this.workspaces.flatMap((workspace) =>
				workspace.terminalTabs.flatMap((tab) =>
					tab.panes.map((pane) => [pane.id, { workspace, tab, pane }])
				)
			)
		)
	);

	/** Follow the server's snapshots; resolves on the first one (or after a while without). */
	load(): Promise<void> {
		this.unsubscribe?.();
		return new Promise<void>((resolve) => {
			const timer = setTimeout(resolve, FIRST_SNAPSHOT_MS);
			this.unsubscribe = this.api.subscribeWorkspace({
				snapshot: (s, fresh) => {
					this.apply(s, fresh);
					clearTimeout(timer);
					resolve();
				}
			});
		});
	}

	dispose(): void {
		this.unsubscribe?.();
		this.unsubscribe = null;
	}

	/** A snapshot from the stream. `fresh`: a new connection, whose `rev` may have restarted. */
	apply(next: WorkspaceSnapshot, fresh = false): void {
		if (!fresh && next.rev <= this.snapshot.rev) return;
		const gone = Object.keys(this.paneIndex);
		const prevProject = this.activeProjectPath;
		this.snapshot = next;
		for (const paneId of gone) if (!(paneId in this.ids)) releaseChat(paneId);
		if (this.seed && next.local) {
			this.seed = false;
			this.setUi(seededUi(next.local));
		}
		this.pending = Object.fromEntries(
			Object.entries(this.pending).filter(([, rev]) => next.rev < rev)
		);
		const pruned = pruneUi(this.ui, next.workspaces, Object.keys(this.pending));
		if (pruned) this.setUi(pruned);
		this.notifySwitch(prevProject);
		for (const { key, notice } of paneNotices(next.workspaces)) {
			if (key in this.shownNotices) continue;
			this.shownNotices[key] = true;
			toast.info(notice);
		}
	}

	private setUi(ui: WorkspaceUi): void {
		this.ui = ui;
		saveUi(ui);
	}

	/** Send one command; a refusal is shown, never thrown. */
	private async send(cmd: WorkspaceCommand): Promise<WorkspaceCommandResult | null> {
		try {
			const result = await this.api.workspaceCommand(cmd);
			for (const id of [result.workspaceId, result.tabId, result.paneId])
				if (id && !(id in this.ids)) this.pending = { ...this.pending, [id]: result.rev };
			return result;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : String(e));
			return null;
		}
	}

	/** Send, then show what the command opened. */
	private async sendAndFocus(cmd: WorkspaceCommand): Promise<WorkspaceCommandResult | null> {
		const result = await this.send(cmd);
		if (result?.workspaceId) {
			const tabs = result.tabId
				? { ...this.ui.activeTabs, [result.workspaceId]: result.tabId }
				: this.ui.activeTabs;
			const prevProject = this.activeProjectPath;
			this.setUi({ ...this.ui, selectedId: result.workspaceId, activeTabs: tabs });
			this.notifySwitch(prevProject);
		}
		return result;
	}

	// --- selection (this device only) ---

	get selectedId(): string | null {
		return this.ui.selectedId;
	}

	set selectedId(id: string | null) {
		const prevProject = this.activeProjectPath;
		this.setUi({ ...this.ui, selectedId: id });
		this.notifySwitch(prevProject);
	}

	private notifySwitch(prevProject: string | null): void {
		const next = this.activeProjectPath;
		if (next && next !== prevProject) for (const cb of this.switchCallbacks) cb(next);
	}

	onWorkspaceSwitch(callback: (projectPath: string) => void): void {
		this.switchCallbacks.push(callback);
	}

	get activeWorkspaceId(): string | null {
		const id = this.ui.selectedId;
		if (id && this.workspaces.some((w) => w.id === id)) return id;
		return this.workspaces[0]?.id ?? null;
	}

	get activeWorkspace(): ProjectWorkspace | null {
		return this.workspaces.find((w) => w.id === this.activeWorkspaceId) ?? null;
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
		return ws?.terminalTabs.find((t) => t.id === ws.activeTerminalTabId) ?? null;
	}

	setActiveTab(workspaceId: string, tabId: string): void {
		if (this.ui.activeTabs[workspaceId] === tabId) return;
		this.setUi({ ...this.ui, activeTabs: { ...this.ui.activeTabs, [workspaceId]: tabId } });
	}

	/** Activate the workspace and tab containing the given pane. */
	focusPane(paneId: string): boolean {
		const at = this.paneIndex[paneId];
		if (!at) return false;
		this.selectedId = at.workspace.id;
		this.setActiveTab(at.workspace.id, at.tab.id);
		return true;
	}

	selectTabByProject(projectPath: string, tabId: string): void {
		const ws = this.workspaces.find(
			(w) => w.projectPath === projectPath && w.terminalTabs.some((t) => t.id === tabId)
		);
		if (!ws) return;
		this.selectedId = ws.id;
		this.setActiveTab(ws.id, tabId);
	}

	/**
	 * Claude: this device's display only, over the one process. Codex: the
	 * server hands the thread between the TUI and `codex app-server`.
	 */
	async setPaneView(paneId: string, view: PaneView): Promise<void> {
		const pane = this.paneIndex[paneId]?.pane;
		if (!pane || (pane.view ?? 'terminal') === view) return;
		if (pane.type === 'codex') {
			await this.send({
				type: 'setCodexMode',
				paneId,
				mode: view === 'chat' ? 'appServer' : 'tui'
			});
			return;
		}
		if (view === 'terminal') releaseChat(paneId);
		const chatPanes = this.ui.chatPanes.filter((id) => id !== paneId);
		this.setUi({ ...this.ui, chatPanes: view === 'chat' ? [...chatPanes, paneId] : chatPanes });
	}

	// --- lookups ---

	pane(paneId: string): TerminalPaneState | undefined {
		return this.paneIndex[paneId]?.pane;
	}

	isProjectOpen(projectPath: string): boolean {
		return this.workspaces.some((w) => w.projectPath === projectPath);
	}

	/** The main workspace (not a worktree) for a project path */
	getByProjectPath(projectPath: string): ProjectWorkspace | undefined {
		return this.workspaces.find((w) => w.projectPath === projectPath && !w.worktreePath);
	}

	getByWorktreePath(worktreePath: string): ProjectWorkspace | undefined {
		return this.workspaces.find((w) => w.worktreePath === worktreePath);
	}

	/** The pane on that session (or one it had before a `/clear`). */
	paneForSession(sessionId: string): TerminalPaneState | undefined {
		for (const { pane } of Object.values(this.paneIndex))
			if (pane.claudeSessionId === sessionId || pane.previousIds?.includes(sessionId)) return pane;
		return undefined;
	}

	/**
	 * `cwd` is the worktree-aware path the pane runs in: session files are keyed
	 * by it, not by the parent project.
	 */
	findAIPaneContext(
		paneId: string,
		type: SessionType = 'claude'
	): { workspaceId: string; tabId: string; projectPath: string; cwd: string } | null {
		const at = this.paneIndex[paneId];
		if (!at || at.tab.type !== type) return null;
		return {
			workspaceId: at.workspace.id,
			tabId: at.tab.id,
			projectPath: at.workspace.projectPath,
			cwd: effectivePath(at.workspace)
		};
	}

	/** Worktrees use their fixed branch; a main checkout's comes from git. */
	resolvedBranch(ws: ProjectWorkspace): string | undefined {
		if (ws.worktreePath) return ws.branch;
		return this.gitStore.branchByProject[ws.projectPath] ?? ws.branch;
	}

	// --- commands ---

	open(project: ProjectConfig): Promise<unknown> {
		return this.sendAndFocus({
			type: 'openWorkspace',
			projectPath: project.path,
			projectName: project.name,
			renderer: this.settingsStore.terminalRenderer
		});
	}

	openWorktree(project: ProjectConfig, worktreePath: string, branch: string): Promise<unknown> {
		return this.sendAndFocus({
			type: 'openWorkspace',
			projectPath: project.path,
			projectName: project.name,
			worktreePath,
			branch,
			renderer: this.settingsStore.terminalRenderer
		});
	}

	close(workspaceId: string): Promise<unknown> {
		return this.send({ type: 'closeWorkspace', workspaceId });
	}

	closeAllForProject(projectPath: string): Promise<unknown> {
		return this.send({ type: 'closeProject', projectPath });
	}

	reorder(fromId: string, toId: string): Promise<unknown> {
		return this.send({ type: 'moveWorkspace', workspaceId: fromId, toWorkspaceId: toId });
	}

	reorderTerminalTab(_workspaceId: string, fromId: string, toId: string): Promise<unknown> {
		return this.send({ type: 'moveTab', tabId: fromId, toTabId: toId });
	}

	addTerminalTab(workspaceId: string): Promise<unknown> {
		return this.sendAndFocus({ type: 'newSession', workspaceId, kind: 'shell' });
	}

	/** `claudeAccountId` runs the task's shell under that Claude login (e.g. `claude auth login`). */
	runTaskInWorkspace(
		workspaceId: string,
		task: ProjectTask,
		claudeAccountId?: string
	): Promise<unknown> {
		return this.sendAndFocus({
			type: 'newSession',
			workspaceId,
			kind: 'shell',
			label: task.name,
			command: task.command,
			...(claudeAccountId && { accountId: claudeAccountId })
		});
	}

	closeTerminalTab(_workspaceId: string, tabId: string): Promise<unknown> {
		return this.send({ type: 'closeTab', tabId });
	}

	closeTabByProject(_projectPath: string, tabId: string): Promise<unknown> {
		return this.send({ type: 'closeTab', tabId });
	}

	removePane(_workspaceId: string, paneId: string): Promise<unknown> {
		return this.send({ type: 'closePane', paneId });
	}

	renameTab(tabId: string, label: string): Promise<unknown> {
		return this.send({ type: 'rename', tabId, label });
	}

	/**
	 * Show the active tab beside its neighbour (a new shell tab when it's the
	 * only one). The same direction again unsplits.
	 */
	splitTerminal(workspaceId: string, direction: SplitDirection): Promise<unknown> {
		const tabId = this.workspaces.find((w) => w.id === workspaceId)?.activeTerminalTabId;
		return tabId ? this.send({ type: 'split', tabId, direction }) : Promise.resolve(null);
	}

	/** A project moved or was renamed: its workspaces follow, on every device. */
	updateProject(
		projectPath: string,
		project: Pick<ProjectConfig, 'path' | 'name'>
	): Promise<unknown> {
		return this.send({
			type: 'updateProject',
			projectPath,
			newPath: project.path,
			projectName: project.name
		});
	}

	trustFolder(paneId: string): Promise<unknown> {
		return this.send({ type: 'trustFolder', paneId });
	}

	/** A new Claude or Codex session in the workspace. */
	addAISession(
		workspaceId: string,
		type: 'claude' | 'codex' = 'claude',
		options?: NewSessionOptions
	): Promise<unknown> {
		return this.newSession({ workspaceId }, type, options);
	}

	/** A new session in the project's main checkout (opened for it if need be). */
	addAIByProject(
		project: Pick<ProjectConfig, 'path' | 'name'>,
		type: 'claude' | 'codex' = 'claude',
		options?: NewSessionOptions
	): Promise<unknown> {
		return this.newSession({ projectPath: project.path, projectName: project.name }, type, options);
	}

	private async newSession(
		target: { workspaceId: string } | { projectPath: string; projectName: string },
		kind: 'claude' | 'codex',
		options?: NewSessionOptions
	): Promise<unknown> {
		const prompt = options?.prompt?.trim();
		const label = options?.label?.trim();
		// A plain new Claude tab can open straight into chat; an agent action's prompt goes to the terminal.
		const asChat = !prompt && this.settingsStore.defaultClaudeView === 'chat';
		const result = await this.sendAndFocus({
			type: 'newSession',
			...target,
			kind,
			...(prompt && { prompt }),
			...(label && { label })
		});
		if (kind === 'claude' && asChat && result?.paneId) this.showAsChat(result.paneId);
		return result;
	}

	/**
	 * Resume a session; the server returns the pane already running it rather
	 * than starting it twice. `accountId` owns the transcript.
	 */
	async resumeAISession(
		workspaceId: string,
		sessionId: string,
		label: string,
		type: 'claude' | 'codex' = 'claude',
		accountId?: string,
		view?: PaneView
	): Promise<unknown> {
		const chat =
			view === 'chat' || (type === 'claude' && this.settingsStore.defaultClaudeView === 'chat');
		const result = await this.sendAndFocus({
			type: 'newSession',
			workspaceId,
			kind: type,
			resume: sessionId,
			label,
			...(accountId && { accountId }),
			...(type === 'codex' && chat && { codexMode: 'appServer' as const })
		});
		if (type === 'claude' && chat && result?.paneId) this.showAsChat(result.paneId);
		return result;
	}

	private showAsChat(paneId: string): void {
		if (!this.ui.chatPanes.includes(paneId))
			this.setUi({ ...this.ui, chatPanes: [...this.ui.chatPanes, paneId] });
	}

	restartAISession(_workspaceId: string, tabId: string): Promise<unknown> {
		return this.send({ type: 'restart', tabId });
	}

	restartClaudeByProject(_projectPath: string, tabId: string): Promise<unknown> {
		return this.send({ type: 'restart', tabId });
	}
}
