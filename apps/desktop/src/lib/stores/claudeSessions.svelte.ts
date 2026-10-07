import { invoke } from '$lib/transport';
import { listen } from '@tauri-apps/api/event';
import { SvelteMap, SvelteSet } from 'svelte/reactivity';
import { stripAnsi } from '$lib/utils/format';
import {
	isAISessionType,
	type ActiveClaudeSession,
	type AgentAttention,
	type AgentAction,
	type AgentSummary,
	type CodexNotifyEvent,
	type DiscoveredClaudeSession,
	type SessionType,
	type TerminalActivityEvent,
	type TerminalDataEvent
} from '$types/workbench';
import type { IntegrationApprovalStore } from './integration-approval.svelte';
import type { WorkspaceStore } from './workspaces.svelte';
import type { ProjectStore } from './projects.svelte';

const SUBMIT_START_FALLBACK_MS = 5000;
/** Quiet window after which a server-hosted Codex pane is marked inactive — mirrors
 *  the native terminal's `terminal:activity` debounce. */
const OUTPUT_QUIESCENCE_MS = 1000;
const LOCAL_ECHO_SUPPRESS_MS = 180;
const LOCAL_ECHO_MAX_CHARS = 4;
const LOCAL_TYPING_SUPPRESS_MS = 2500;
const LOCAL_VIEWPORT_SUPPRESS_MS = 700;
/** Bound on session-label discovery retries, so a session that genuinely never gets a
 *  label (no user message on disk) stops rescanning the session directory. */
const MAX_LABEL_DISCOVERY_ATTEMPTS = 6;

/** Who to notify about: a pane here, or a session no pane shows (started on the phone). */
export type AttentionTarget =
	| { paneId: string; sessionId: string }
	| { id: string; projectPath: string; label: string };

export class ClaudeSessionStore {
	/** Set of terminal pane IDs currently producing output (clears from backend activity events) */
	panesInProgress: SvelteSet<string> = $state(new SvelteSet());

	/** Set of terminal pane IDs where Claude is blocked waiting for user action (permission/question) */
	panesAwaitingInput: SvelteSet<string> = $state(new SvelteSet());

	/** Cached discovered Claude sessions for the current project */
	discoveredSessions: DiscoveredClaudeSession[] = $state([]);

	/** Cached discovered Codex sessions for the current project */
	discoveredCodexSessions: DiscoveredClaudeSession[] = $state([]);

	/** Per-pane fallback timeout if no output arrives after Enter submit */
	private submitFallbackTimeouts = new SvelteMap<string, ReturnType<typeof setTimeout>>();
	/** Per-pane debounce that marks a server-hosted Codex pane inactive after a quiet
	 *  window (replaces the local `terminal:activity` event for WS-backed panes). */
	private outputQuiescenceTimers = new SvelteMap<string, ReturnType<typeof setTimeout>>();
	/** Last timestamp when local user input was sent to a pane */
	private lastLocalInputAt = new SvelteMap<string, number>();
	/** Last timestamp when user typed non-submit input (no Enter/newline) */
	private lastTypingInputAt = new SvelteMap<string, number>();
	/** Last timestamp when local viewport change occurred (resize/visibility resume) */
	private lastViewportChangeAt = new SvelteMap<string, number>();
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

	/** Start a new AI session. Claude session identity is hook-driven. */
	async startSession(workspaceId: string, type: SessionType = 'claude') {
		if (!(await this.integrationApproval.ensureIntegration(type))) return;
		this.workspaces.addAISession(workspaceId, type);
	}

	/** Start an AI session for a project (opens project if needed) */
	async startSessionByProject(projectPath: string, type: SessionType = 'claude') {
		if (!(await this.integrationApproval.ensureIntegration(type))) return;
		this.projects.openProject(projectPath);
		this.workspaces.addAIByProject(projectPath, type);
	}

	/** Start an agent action for a project (opens project/workspace if needed). */
	async startAgentActionByProject(
		projectPath: string,
		action: AgentAction,
		type: 'claude' | 'codex'
	) {
		if (!(await this.integrationApproval.ensureIntegration(type))) return;
		this.projects.openProject(projectPath);
		this.workspaces.addAIByProject(projectPath, type, {
			label: action.name,
			prompt: action.prompt
		});
	}

	/** Resume an existing AI session, gated through integration approval */
	async resumeSession(
		workspaceId: string,
		sessionId: string,
		label: string,
		type: SessionType = 'claude'
	) {
		if (!(await this.integrationApproval.ensureIntegration(type))) return;
		const accountId =
			type === 'claude'
				? this.discoveredSessions.find((s) => s.sessionId === sessionId)?.accountId
				: undefined;
		this.workspaces.resumeAISession(workspaceId, sessionId, label, type, accountId);
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
		this.workspaces.addAISession(ws.id, type, { label: action.name, prompt: action.prompt });
	}

	private getAIPaneId(tab: {
		type?: SessionType;
		panes: { id: string; type?: SessionType }[];
	}): string | null {
		if (tab.type !== 'claude' && tab.type !== 'codex') return null;
		const typedPane = tab.panes.find((p) => p.type === tab.type);
		if (typedPane) return typedPane.id;
		// Legacy snapshots may be missing pane.type; AI pane is the original first pane.
		return tab.panes[0]?.id ?? null;
	}

	paneType(paneId: string): SessionType | null {
		return this.paneTypeById[paneId] ?? null;
	}

	/** Mark that local keyboard input was sent for a pane (used to suppress echoed characters). */
	noteLocalInput(paneId: string, data: string): void {
		if (this.paneType(paneId) !== 'codex') return;
		const now = Date.now();
		this.lastLocalInputAt.set(paneId, now);
		const isEnterSubmit = data.includes('\r');
		if (isEnterSubmit) {
			// Start activity immediately on Enter submit. Shift+Enter sends '\n' only and should not trigger.
			this.panesInProgress.add(paneId);
			const existing = this.submitFallbackTimeouts.get(paneId);
			if (existing) clearTimeout(existing);
			this.submitFallbackTimeouts.set(
				paneId,
				setTimeout(() => {
					this.panesInProgress.delete(paneId);
					this.submitFallbackTimeouts.delete(paneId);
				}, SUBMIT_START_FALLBACK_MS)
			);
			this.lastTypingInputAt.delete(paneId);
			return;
		}
		if (data.includes('\n')) {
			this.lastTypingInputAt.delete(paneId);
			return;
		}
		this.lastTypingInputAt.set(paneId, now);
	}

	/** Mark that local viewport changed (e.g. resize/visibility), which can trigger prompt redraw noise. */
	noteLocalViewportChange(paneId: string): void {
		if (this.paneType(paneId) !== 'codex') return;
		this.lastViewportChangeAt.set(paneId, Date.now());
	}

	/**
	 * Feed PTY output for a pane through the same activity/quiescence logic the
	 * `terminal:data` + `terminal:activity` Tauri events drive for native terminal
	 * panes. Server-hosted xterm panes stream output over the WebSocket and never
	 * emit those events, so TerminalPane calls this directly to keep Codex
	 * in-progress/quiescence working. Claude panes don't need it: their state
	 * comes from the plugin (`claude:hook`, `agent:attention`).
	 */
	noteTerminalOutput(paneId: string, data: string): void {
		// Replicate terminal:activity: mark inactive after a quiet window with no
		// further output (the WS path has no backend activity debounce).
		if (this.noteOutput(paneId, data)) this.scheduleOutputQuiescence(paneId);
	}

	/** Real Codex output (not echo or redraw) marks the pane active; true if it did. */
	private noteOutput(paneId: string, data: string): boolean {
		if (this.paneType(paneId) !== 'codex') return false;
		if (this.classifyTerminalData(paneId, data)) return false;
		this.panesInProgress.add(paneId);
		this.clearSubmitFallback(paneId);
		return true;
	}

	/** Reset the per-pane quiescence debounce; on fire, mark the Codex pane inactive. */
	private scheduleOutputQuiescence(paneId: string): void {
		const existing = this.outputQuiescenceTimers.get(paneId);
		if (existing) clearTimeout(existing);
		this.outputQuiescenceTimers.set(
			paneId,
			setTimeout(() => {
				this.outputQuiescenceTimers.delete(paneId);
				if (this.paneType(paneId) !== 'codex') return;
				this.panesInProgress.delete(paneId);
				this.clearSubmitFallback(paneId);
			}, OUTPUT_QUIESCENCE_MS)
		);
	}

	private classifyTerminalData(paneId: string, data: string): boolean {
		const now = Date.now();
		const plain = stripAnsi(data).replace(/\r/g, '');
		if (plain.trim().length === 0) {
			return true;
		}

		const viewportChangeAt = this.lastViewportChangeAt.get(paneId);
		if (viewportChangeAt && now - viewportChangeAt <= LOCAL_VIEWPORT_SUPPRESS_MS) {
			return true;
		}

		const typingInputAt = this.lastTypingInputAt.get(paneId);
		if (typingInputAt && now - typingInputAt <= LOCAL_TYPING_SUPPRESS_MS) {
			// While user is typing (without submit), treat any incoming redraw/echo as local UI churn.
			return true;
		}

		const lastInputAt = this.lastLocalInputAt.get(paneId);
		if (!lastInputAt) return false;
		if (now - lastInputAt > LOCAL_ECHO_SUPPRESS_MS) {
			return false;
		}

		if (plain === '\n' || (!plain.includes('\n') && plain.length <= LOCAL_ECHO_MAX_CHARS)) {
			return true;
		}
		return false;
	}

	private clearSubmitFallback(paneId: string): void {
		const fallback = this.submitFallbackTimeouts.get(paneId);
		if (fallback) {
			clearTimeout(fallback);
			this.submitFallbackTimeouts.delete(paneId);
		}
	}

	/**
	 * A chat session on this machine (Claude via the plugin's mod link, or a
	 * Codex chat) started or stopped waiting on someone, or finished a turn: the
	 * same server events the phone consumes, so both devices notify.
	 */
	private onAgentAttention(event: AgentAttention): void {
		const paneId = this.workspaces.paneForAgent(event);
		if (!paneId) {
			// Not open here (yet): a session started on the phone.
			const label = event.title ?? `Session ${event.sessionId.slice(0, 8)}`;
			this.emitAttention(
				{ id: event.sessionId, projectPath: event.projectPath, label },
				event.kind
			);
			return;
		}
		switch (event.kind) {
			case 'waiting':
				this.panesInProgress.delete(paneId);
				this.panesAwaitingInput.add(paneId);
				break;
			case 'resolved':
				this.panesAwaitingInput.delete(paneId);
				if (event.busy) this.panesInProgress.add(paneId);
				break;
			case 'turnEnded':
				this.panesInProgress.delete(paneId);
				this.panesAwaitingInput.delete(paneId);
				break;
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

	/**
	 * Claude panes follow the server's summaries, as the phone does: every Claude
	 * terminal's plugin (xterm, native and chat) feeds them, so the tab label is
	 * the session's current title and the busy state its turn. A pane follows
	 * its `claude` onto another session (`/resume` in the TUI); a chat pane's id
	 * comes only from its chat.
	 */
	syncFromAgents(list: AgentSummary[]): void {
		const newest: Record<string, AgentSummary> = {};
		for (const a of list) {
			if (a.agent !== 'claude' || a.exited) continue;
			const paneId = this.workspaces.paneForAgent(a);
			if (!paneId || this.paneType(paneId) !== 'claude') continue;
			if ((newest[paneId]?.updatedAt ?? -Infinity) < a.updatedAt) newest[paneId] = a;
		}
		for (const [paneId, type] of Object.entries(this.paneTypeById)) {
			if (type !== 'claude') continue;
			const a = newest[paneId];
			// Not busy while it waits on someone (that's `panesAwaitingInput`), or once gone.
			if (a?.busy && !a.waiting) this.panesInProgress.add(paneId);
			else this.panesInProgress.delete(paneId);
			if (!a) continue;
			if (a.paneId === paneId && !this.workspaces.isChatPane(paneId)) {
				this.workspaces.updateAISessionByPaneId(paneId, a.sessionId, 'claude');
			}
			if (a.title) this.workspaces.updateAITabLabelByPaneId(paneId, a.title, 'claude');
		}
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
			this.workspaces.updateAITabLabelByPaneId(paneId, cached, type);
			return;
		}

		this.workspaces.updateAITabLabelByPaneId(paneId, fallback, type);

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
			this.workspaces.updateAITabLabelByPaneId(paneId, match.label, type);
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
		if (this.paneType(paneId) !== 'codex') return;

		if (event.sessionId) {
			// A chat pane's thread id comes only from its chat, as for Claude.
			if (!this.workspaces.isChatPane(paneId)) {
				this.workspaces.updateAISessionByPaneId(paneId, event.sessionId, 'codex');
			}
			this.latestCodexSessionByPane.set(paneId, event.sessionId);
			void this.syncLabelFromSession(
				paneId,
				event.sessionId,
				event.notifyEvent === 'agent-turn-complete'
			);
		}

		// The bridge publishes this completion to the shared attention feed;
		// this event updates labels/activity without raising a second alert.
		if (event.notifyEvent === 'agent-turn-complete') {
			this.panesInProgress.delete(paneId);
			this.clearSubmitFallback(paneId);
		}
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

		// Local PTYs: `terminal:activity` below does the quiescence.
		listen<TerminalDataEvent>('terminal:data', (event) => {
			this.noteOutput(event.payload.sessionId, event.payload.data);
		});

		listen<TerminalActivityEvent>('terminal:activity', (event) => {
			const paneId = event.payload.sessionId;
			if (event.payload.active) return;
			if (this.paneType(paneId) !== 'codex') return;
			this.panesInProgress.delete(paneId);
			this.clearSubmitFallback(paneId);
		});
	}
}
