import { invoke } from '$lib/transport';
import { listen } from '@tauri-apps/api/event';
import { SvelteMap, SvelteSet } from 'svelte/reactivity';
import {
	isAISessionType,
	type ActiveClaudeSession,
	type AgentAttention,
	type AgentAction,
	type CodexNotifyEvent,
	type DiscoveredClaudeSession,
	type SessionType
} from '$types/workbench';
import type { IntegrationApprovalStore } from './integration-approval.svelte';
import type { WorkspaceStore } from './workspaces.svelte';
import type { ProjectStore } from './projects.svelte';

/** Bound on session-label discovery retries, so a session that genuinely never gets a
 *  label (no user message on disk) stops rescanning the session directory. */
const MAX_LABEL_DISCOVERY_ATTEMPTS = 6;

/** Who to notify about: a pane here, or a session no pane shows. */
export type AttentionTarget =
	| { paneId: string; sessionId: string }
	| { id: string; projectPath: string; label: string };

export class ClaudeSessionStore {
	/** AI panes mid-turn and not waiting on anyone (the server's runtime state). */
	readonly panesInProgress: ReadonlySet<string> = $derived(
		new SvelteSet(this.aiPanes().flatMap((p) => (p.busy && !p.waiting ? [p.id] : [])))
	);

	/** AI panes blocked on someone (an approval or a question). */
	readonly panesAwaitingInput: ReadonlySet<string> = $derived(
		new SvelteSet(this.aiPanes().flatMap((p) => (p.waiting ? [p.id] : [])))
	);

	/** Cached discovered Claude sessions for the current project */
	discoveredSessions: DiscoveredClaudeSession[] = $state([]);

	/** Cached discovered Codex sessions for the current project */
	discoveredCodexSessions: DiscoveredClaudeSession[] = $state([]);

	/** Latest Codex session ID observed for each pane from notify events */
	private latestCodexSessionByPane = new SvelteMap<string, string>();
	/** Cache of sessionId → resolved label. Only ever holds labels we actually found. */
	private resolvedSessionLabels = new SvelteMap<string, string>();
	/** Discovery attempts per sessionId, so an unlabelled session retries but stays bounded. */
	private labelDiscoveryAttempts = new SvelteMap<string, number>();
	/** Reference to workspace store for Claude pane detection */
	private workspaces: WorkspaceStore;
	/** Reference to project store for opening projects */
	private projects: ProjectStore;
	/** Reference to integration approval store for gating AI sessions */
	private integrationApproval: IntegrationApprovalStore;

	/** Callbacks for a session needing someone (an answer, or its turn ended), or no longer */
	private attentionCallbacks: Array<
		(target: AttentionTarget, kind: AgentAttention['kind']) => void
	> = [];

	/** Active Claude sessions grouped by project path */
	readonly activeSessionsByProject = $derived.by((): Record<string, ActiveClaudeSession[]> => {
		return this.workspaces.workspaces.reduce<Record<string, ActiveClaudeSession[]>>((acc, ws) => {
			const sessions = ws.terminalTabs
				.filter((t) => isAISessionType(t.type))
				.map((t) => {
					const sessionType: ActiveClaudeSession['sessionType'] =
						t.type === 'codex' ? 'codex' : 'claude';
					const aiPaneId = this.getAIPaneId(t);
					const aiPane = aiPaneId ? t.panes.find((p) => p.id === aiPaneId) : null;
					return {
						claudeSessionId: aiPane?.claudeSessionId ?? '',
						tabId: t.id,
						label: t.label,
						sessionType,
						needsAttention: aiPaneId ? !this.panesInProgress.has(aiPaneId) : true,
						awaitingInput: aiPaneId ? this.panesAwaitingInput.has(aiPaneId) : false,
						worktreePath: ws.worktreePath
					};
				});
			if (sessions.length > 0) {
				acc[ws.projectPath] = [...(acc[ws.projectPath] ?? []), ...sessions];
			}
			return acc;
		}, {});
	});

	/** Global active-session counts by agent type, across all projects. */
	readonly totalSessionCounts = $derived.by((): { claude: number; codex: number } => {
		let claude = 0;
		let codex = 0;
		for (const sessions of Object.values(this.activeSessionsByProject)) {
			for (const s of sessions) {
				if (s.sessionType === 'codex') codex++;
				else claude++;
			}
		}
		return { claude, codex };
	});

	readonly paneTypeById = $derived.by((): Record<string, SessionType> => {
		const result: Record<string, SessionType> = {};
		for (const ws of this.workspaces.workspaces) {
			for (const tab of ws.terminalTabs) {
				if (!isAISessionType(tab.type)) continue;
				const paneId = this.getAIPaneId(tab);
				if (paneId) result[paneId] = tab.type;
			}
		}
		return result;
	});

	/** Read Claude CLI session files from ~/.claude/projects/ (the resume list). */
	async discoverSessions(projectPath: string): Promise<DiscoveredClaudeSession[]> {
		return (this.discoveredSessions = await this.peekSessions(projectPath, 'claude'));
	}

	/** Read Codex session files from ~/.codex/sessions/ filtered by cwd (the resume list). */
	async discoverCodexSessions(projectPath: string): Promise<DiscoveredClaudeSession[]> {
		return (this.discoveredCodexSessions = await this.peekSessions(projectPath, 'codex'));
	}

	/** Remove a session from the discovered list (does not delete the JSONL file) */
	removeDiscoveredSession(sessionId: string, type: SessionType = 'claude'): void {
		if (type === 'codex') {
			this.discoveredCodexSessions = this.discoveredCodexSessions.filter(
				(s) => s.sessionId !== sessionId
			);
		} else {
			this.discoveredSessions = this.discoveredSessions.filter((s) => s.sessionId !== sessionId);
		}
	}

	/** Start a new AI session in a workspace. */
	async startSession(workspaceId: string, type: 'claude' | 'codex' = 'claude') {
		if (!(await this.integrationApproval.ensureIntegration(type))) return;
		await this.workspaces.addAISession(workspaceId, type);
	}

	/** Start an AI session in a project's main checkout (opened if needed). */
	async startSessionByProject(projectPath: string, type: 'claude' | 'codex' = 'claude') {
		const project = this.projects.getByPath(projectPath);
		if (!project || !(await this.integrationApproval.ensureIntegration(type))) return;
		await this.workspaces.addAIByProject(project, type);
	}

	/** Start an agent action for a project (opens project/workspace if needed). */
	async startAgentActionByProject(
		projectPath: string,
		action: AgentAction,
		type: 'claude' | 'codex'
	) {
		const project = this.projects.getByPath(projectPath);
		if (!project || !(await this.integrationApproval.ensureIntegration(type))) return;
		await this.workspaces.addAIByProject(project, type, {
			label: action.name,
			prompt: action.prompt
		});
	}

	/** Resume a session; the server shows the pane already running it instead of a second one. */
	async resumeSession(
		workspaceId: string,
		sessionId: string,
		label: string,
		type: 'claude' | 'codex' = 'claude'
	) {
		if (!(await this.integrationApproval.ensureIntegration(type))) return;
		const accountId =
			type === 'claude'
				? this.discoveredSessions.find((s) => s.sessionId === sessionId)?.accountId
				: undefined;
		await this.workspaces.resumeAISession(workspaceId, sessionId, label, type, accountId);
	}

	/** Restart an AI session, gated through integration approval */
	async restartSession(workspaceId: string, tabId: string, type: SessionType = 'claude') {
		if (!(await this.integrationApproval.ensureIntegration(type))) return;
		await this.workspaces.restartAISession(workspaceId, tabId);
	}

	/** Restart an AI session by project path, gated through integration approval */
	async restartSessionByProject(projectPath: string, tabId: string, type: SessionType = 'claude') {
		if (!(await this.integrationApproval.ensureIntegration(type))) return;
		await this.workspaces.restartClaudeByProject(projectPath, tabId);
	}

	/** Start an agent action in a specific workspace with an auto-submitted initial prompt. */
	async startAgentActionInWorkspace(
		ws: { id: string },
		action: AgentAction,
		type: 'claude' | 'codex'
	) {
		if (!(await this.integrationApproval.ensureIntegration(type))) return;
		await this.workspaces.addAISession(ws.id, type, { label: action.name, prompt: action.prompt });
	}

	private getAIPaneId(tab: {
		type?: SessionType;
		panes: { id: string; type?: SessionType }[];
	}): string | null {
		if (!isAISessionType(tab.type)) return null;
		return (tab.panes.find((p) => p.type === tab.type) ?? tab.panes[0])?.id ?? null;
	}

	private aiPanes() {
		return this.workspaces.workspaces.flatMap((w) =>
			w.terminalTabs.flatMap((t) => (isAISessionType(t.type) ? t.panes : []))
		);
	}

	paneType(paneId: string): SessionType | null {
		return this.paneTypeById[paneId] ?? null;
	}

	/**
	 * A session on this machine started or stopped waiting on someone, or
	 * finished a turn: the same server events the phone consumes, so both
	 * devices notify. Busy and waiting themselves come with the snapshot.
	 */
	private onAgentAttention(event: AgentAttention): void {
		const paneId =
			(event.paneId && this.workspaces.pane(event.paneId)?.id) ||
			this.workspaces.paneForSession(event.sessionId)?.id;
		if (!paneId) {
			const label = event.title ?? `Session ${event.sessionId.slice(0, 8)}`;
			this.emitAttention(
				{ id: event.sessionId, projectPath: event.projectPath, label },
				event.kind
			);
			return;
		}
		this.emitAttention({ paneId, sessionId: event.sessionId }, event.kind);
	}

	/** Register a callback for a session that needs someone, or no longer does (`resolved`). */
	onAttention(callback: (target: AttentionTarget, kind: AgentAttention['kind']) => void): void {
		this.attentionCallbacks.push(callback);
	}

	private emitAttention(target: AttentionTarget, kind: AgentAttention['kind']): void {
		for (const cb of this.attentionCallbacks) {
			try {
				cb(target, kind);
			} catch (e) {
				console.warn('[ClaudeSessionStore] attention callback error:', e);
			}
		}
	}

	/** A Codex TUI's tab takes its thread's first message, saved on the server as its label. */
	private relabel(paneId: string, label: string): void {
		const tab = this.workspaces.workspaces
			.flatMap((w) => w.terminalTabs)
			.find((t) => t.type === 'codex' && t.panes.some((p) => p.id === paneId));
		if (tab && tab.label !== label) void this.workspaces.renameTab(tab.id, label);
	}

	/** A Codex pane's label is its thread's first message, from its session file. */
	private async syncLabelFromSession(
		paneId: string,
		sessionId: string,
		allowRetry = false
	): Promise<void> {
		const type = 'codex';
		const fallback = `Session ${sessionId.slice(0, 8)}`;

		// Check cache: if we already resolved a real label for this session, just apply it.
		const cached = this.resolvedSessionLabels.get(sessionId);
		if (cached) {
			this.relabel(paneId, cached);
			return;
		}

		// A session's label is its first user message, which doesn't exist yet when the
		// first hook (SessionStart) fires — so the initial discovery always misses.
		// Caching that miss permanently left every tab showing `Session a1b2c3d4` for the
		// rest of the session. Retry on later events that could have created the label,
		// bounded so an genuinely unlabelled session doesn't rescan forever.
		const attempts = this.labelDiscoveryAttempts.get(sessionId) ?? 0;
		if (attempts > 0 && !allowRetry) return;
		if (attempts >= MAX_LABEL_DISCOVERY_ATTEMPTS) return;
		this.labelDiscoveryAttempts.set(sessionId, attempts + 1);

		const ctx = this.workspaces.findAIPaneContext(paneId, type);
		if (!ctx) return;

		// Read-only lookup: `discoverSessions` also overwrites the store-wide resume
		// list, which a retry loop would yank out from under another project's landing
		// page. Label sync must not have that side effect.
		const sessions = await this.peekSessions(ctx.cwd, type);
		if (this.latestCodexSessionByPane.get(paneId) !== sessionId) return;

		// The backend substitutes `Session <id>` when the JSONL has no user message yet
		// (`session_utils::fallback_label`), so a label equal to our own fallback means
		// "not named yet" — treating it as resolved is what made this retry inert.
		const match = sessions.find((s) => s.sessionId === sessionId);
		if (match?.label && match.label !== fallback) {
			this.resolvedSessionLabels.set(sessionId, match.label);
			this.labelDiscoveryAttempts.delete(sessionId);
			this.relabel(paneId, match.label);
		}
	}

	/** Sessions in `cwd` without touching the store-wide resume list. */
	async peekSessions(cwd: string, type: 'claude' | 'codex'): Promise<DiscoveredClaudeSession[]> {
		const command = type === 'codex' ? 'discover_codex_sessions' : 'discover_claude_sessions';
		try {
			return await invoke<DiscoveredClaudeSession[]>(command, { projectPath: cwd });
		} catch (e) {
			console.error(`[ClaudeSessionStore] Failed to discover ${type} sessions:`, e);
			return [];
		}
	}

	private onCodexNotifyEvent(event: CodexNotifyEvent): void {
		const paneId = event.paneId;
		if (this.paneType(paneId) !== 'codex' || !event.sessionId) return;
		this.latestCodexSessionByPane.set(paneId, event.sessionId);
		void this.syncLabelFromSession(
			paneId,
			event.sessionId,
			event.notifyEvent === 'agent-turn-complete'
		);
	}

	constructor(
		workspaces: WorkspaceStore,
		projects: ProjectStore,
		integrationApproval: IntegrationApprovalStore
	) {
		this.workspaces = workspaces;
		this.projects = projects;
		this.integrationApproval = integrationApproval;

		listen<CodexNotifyEvent>('codex:notify', (event) => {
			this.onCodexNotifyEvent(event.payload);
		});
		listen<AgentAttention>('agent:attention', (event) => {
			this.onAgentAttention(event.payload);
		});
	}
}
