import { invoke } from '$lib/transport';
import { listen } from '@tauri-apps/api/event';
import { SvelteMap, SvelteSet } from 'svelte/reactivity';
import { KEEPALIVE_PROMPT } from '@workbench/chat-ui';
import { stripAnsi } from '$lib/utils/format';
import { newSessionCommandWithPrompt, type LaunchOptions } from '$lib/utils/claude';
import { getWorkbenchSettingsStore } from './context';
import {
	isAISessionType,
	type ActiveClaudeSession,
	type AgentAttention,
	type AgentAction,
	type ClaudeHookEvent,
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
 *  the local PtyManager `terminal:activity` debounce (TERMINAL_QUIET_THRESHOLD_MS). */
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
	| { paneId: string }
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
	/** Latest Claude session ID observed for each pane from hook events */
	private latestClaudeSessionByPane = new SvelteMap<string, string>();
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
	/** Reference to workbench settings store */
	private settingsStore = getWorkbenchSettingsStore();

	private get launchOptions(): LaunchOptions {
		return this.settingsStore.launchOptions;
	}

	/** Callbacks invoked when a session needs someone (an answer, or its turn ended) */
	private awaitingInputCallbacks: Array<(target: AttentionTarget) => void> = [];

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

	/** Read Claude CLI session files from ~/.claude/projects/ */
	async discoverSessions(projectPath: string): Promise<DiscoveredClaudeSession[]> {
		try {
			const sessions = await invoke<DiscoveredClaudeSession[]>('discover_claude_sessions', {
				projectPath
			});
			this.discoveredSessions = sessions;
			return sessions;
		} catch (e) {
			console.error('[ClaudeSessionStore] Failed to discover sessions:', e);
			return [];
		}
	}

	/** Read Codex session files from ~/.codex/sessions/ filtered by cwd */
	async discoverCodexSessions(projectPath: string): Promise<DiscoveredClaudeSession[]> {
		try {
			const sessions = await invoke<DiscoveredClaudeSession[]>('discover_codex_sessions', {
				projectPath
			});
			this.discoveredCodexSessions = sessions;
			return sessions;
		} catch (e) {
			console.error('[ClaudeSessionStore] Failed to discover Codex sessions:', e);
			return [];
		}
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
	startAgentActionByProject(projectPath: string, action: AgentAction, type: 'claude' | 'codex') {
		this.projects.openProject(projectPath);
		this.workspaces.addAIByProject(projectPath, type, {
			label: action.name,
			startupCommand: newSessionCommandWithPrompt(type, action.prompt, this.launchOptions)
		});
	}

	/** Start an AI session in a specific workspace */
	async startSessionInWorkspace(
		ws: { id: string; projectPath: string; worktreePath?: string },
		type: SessionType = 'claude'
	) {
		await this.startSession(ws.id, type);
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
	startAgentActionInWorkspace(
		ws: { id: string; projectPath: string; worktreePath?: string },
		action: AgentAction,
		type: 'claude' | 'codex'
	) {
		this.workspaces.addAISession(ws.id, type, {
			label: action.name,
			startupCommand: newSessionCommandWithPrompt(type, action.prompt, this.launchOptions)
		});
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
	 * `terminal:data` + `terminal:activity` Tauri events drive for local PtyManager
	 * panes. Server-hosted xterm panes stream output over the WebSocket and never
	 * emit those events, so TerminalPane calls this directly to keep Codex
	 * in-progress/quiescence working. Claude panes don't need it: their state
	 * comes from the plugin (`claude:hook`, `agent:attention`).
	 */
	noteTerminalOutput(paneId: string, data: string): void {
		if (this.paneType(paneId) !== 'codex') return;
		if (this.classifyTerminalData(paneId, data)) return;
		// Real output → mark active and clear the submit fallback.
		this.panesInProgress.add(paneId);
		this.clearSubmitFallback(paneId);
		// Replicate terminal:activity: mark inactive after a quiet window with no
		// further output (the WS path has no backend activity debounce).
		this.scheduleOutputQuiescence(paneId);
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

	private payloadString(payload: Record<string, unknown>, key: string): string {
		const value = payload[key];
		return typeof value === 'string' ? value : '';
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
	 * same server state the phone's notifications poll, so both devices notify.
	 */
	private onAgentAttention(event: AgentAttention): void {
		const paneId = this.workspaces.paneForAgent(event);
		if (!paneId) {
			// Not open here (yet): a session started on the phone.
			if (event.kind === 'resolved') return;
			const label = event.title ?? `Session ${event.sessionId.slice(0, 8)}`;
			this.emitAwaitingInput({ id: event.sessionId, projectPath: event.projectPath, label });
			return;
		}
		switch (event.kind) {
			case 'waiting':
				this.panesInProgress.delete(paneId);
				this.panesAwaitingInput.add(paneId);
				this.emitAwaitingInput({ paneId });
				break;
			case 'resolved':
				this.panesAwaitingInput.delete(paneId);
				break;
			case 'turnEnded':
				this.panesInProgress.delete(paneId);
				this.panesAwaitingInput.delete(paneId);
				this.emitAwaitingInput({ paneId });
				break;
		}
	}

	/** Register a callback that fires when a session needs someone. */
	onAwaitingInput(callback: (target: AttentionTarget) => void): void {
		this.awaitingInputCallbacks.push(callback);
	}

	private emitAwaitingInput(target: AttentionTarget): void {
		for (const cb of this.awaitingInputCallbacks) {
			try {
				cb(target);
			} catch (e) {
				console.warn('[ClaudeSessionStore] awaiting-input callback error:', e);
			}
		}
	}

	private onClaudeHookEvent(event: ClaudeHookEvent): void {
		const paneId = event.paneId;
		if (this.paneType(paneId) !== 'claude') return;

		if (event.sessionId) {
			// A chat pane's session id comes only from its chat: the hook can report
			// `/clear`'s new id before the server has moved the process to it, and
			// the pane would then start a second claude on an id already in use.
			if (!this.workspaces.isChatPane(paneId)) {
				this.workspaces.updateAISessionByPaneId(paneId, event.sessionId, 'claude');
			}
			this.latestClaudeSessionByPane.set(paneId, event.sessionId);
			// Only these two can have produced a first user message; retrying on every
			// hook would rescan the session directory on each PostToolUse.
			const canRetryLabel =
				event.hookEventName === 'UserPromptSubmit' || event.hookEventName === 'Stop';
			void this.syncLabelFromSession(paneId, event.sessionId, 'claude', canRetryLabel);
		}

		switch (event.hookEventName) {
			case 'UserPromptSubmit':
				// A cache keep-alive turn isn't work to report: its Stop then flags nothing.
				if (this.payloadString(event.hookPayload, 'prompt') === KEEPALIVE_PROMPT) break;
				this.panesInProgress.add(paneId);
				this.panesAwaitingInput.delete(paneId);
				break;
			// What needs someone (and the notification) comes from `agent:attention`.
			case 'Stop':
			case 'SessionStart':
				this.panesInProgress.delete(paneId);
				this.panesAwaitingInput.delete(paneId);
				break;
		}
	}

	private async syncLabelFromSession(
		paneId: string,
		sessionId: string,
		type: 'claude' | 'codex',
		allowRetry = false
	): Promise<void> {
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

		const latestByPane =
			type === 'codex' ? this.latestCodexSessionByPane : this.latestClaudeSessionByPane;
		// Read-only lookup: `discoverSessions` also overwrites the store-wide resume
		// list, which a retry loop would yank out from under another project's landing
		// page. Label sync must not have that side effect.
		const sessions = await this.peekSessions(ctx.cwd, type);
		if (latestByPane.get(paneId) !== sessionId) return;

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

	/** Discover sessions without touching the shared `discovered*Sessions` state. */
	/** Sessions in `cwd` without touching the store-wide resume list. */
	async peekSessions(cwd: string, type: 'claude' | 'codex'): Promise<DiscoveredClaudeSession[]> {
		const command = type === 'codex' ? 'discover_codex_sessions' : 'discover_claude_sessions';
		try {
			return await invoke<DiscoveredClaudeSession[]>(command, { projectPath: cwd });
		} catch (e) {
			console.error('[ClaudeSessionStore] Failed to discover sessions for label:', e);
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
				'codex',
				event.notifyEvent === 'agent-turn-complete'
			);
		}

		// Codex notify currently delivers completion/approval style events.
		if (event.notifyEvent === 'agent-turn-complete') {
			const wasInProgress = this.panesInProgress.has(paneId);
			this.panesInProgress.delete(paneId);
			this.clearSubmitFallback(paneId);
			if (wasInProgress) this.emitAwaitingInput({ paneId });
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

		listen<ClaudeHookEvent>('claude:hook', (event) => {
			this.onClaudeHookEvent(event.payload);
		});
		listen<CodexNotifyEvent>('codex:notify', (event) => {
			this.onCodexNotifyEvent(event.payload);
		});
		listen<AgentAttention>('agent:attention', (event) => {
			this.onAgentAttention(event.payload);
		});

		listen<TerminalDataEvent>('terminal:data', (event) => {
			const paneId = event.payload.sessionId;
			if (this.paneType(paneId) !== 'codex') return;
			if (this.classifyTerminalData(paneId, event.payload.data)) return;

			// Output received — mark as active and clear submit fallback.
			this.panesInProgress.add(paneId);
			this.clearSubmitFallback(paneId);
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
