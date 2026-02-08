import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { SvelteMap, SvelteSet } from 'svelte/reactivity';
import { stripAnsi } from '$lib/utils/format';
import type {
	ActiveClaudeSession,
	ClaudeHookEvent,
	CodexNotifyEvent,
	DiscoveredClaudeSession,
	SessionType,
	TerminalDataEvent
} from '$types/workbench';
import type { WorkspaceStore } from './workspaces.svelte';
import type { ProjectStore } from './projects.svelte';

const QUIET_THRESHOLD_MS = 1000;
const SUBMIT_START_FALLBACK_MS = 5000;
const LOCAL_ECHO_SUPPRESS_MS = 180;
const LOCAL_ECHO_MAX_CHARS = 4;
const LOCAL_TYPING_SUPPRESS_MS = 2500;
const LOCAL_VIEWPORT_SUPPRESS_MS = 700;

export class ClaudeSessionStore {
	/** Set of terminal pane IDs currently producing output (clears after QUIET_THRESHOLD_MS) */
	panesInProgress: SvelteSet<string> = $state(new SvelteSet());

	/** Cached discovered Claude sessions for the current project */
	discoveredSessions: DiscoveredClaudeSession[] = $state([]);

	/** Cached discovered Codex sessions for the current project */
	discoveredCodexSessions: DiscoveredClaudeSession[] = $state([]);

	/** Per-pane debounce timeouts for quiescence detection */
	private quiesceTimeouts = new SvelteMap<string, ReturnType<typeof setTimeout>>();
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
	/** Reference to workspace store for Claude pane detection */
	private workspaces: WorkspaceStore;
	/** Reference to project store for opening projects */
	private projects: ProjectStore;

	/** Active Claude sessions grouped by project path */
	get activeSessionsByProject(): Record<string, ActiveClaudeSession[]> {
		return this.workspaces.workspaces.reduce<Record<string, ActiveClaudeSession[]>>((acc, ws) => {
			const sessions = ws.terminalTabs
				.filter((t) => t.type === 'claude' || t.type === 'codex')
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
						worktreePath: ws.worktreePath
					};
				});
			if (sessions.length > 0) {
				acc[ws.projectPath] = [...(acc[ws.projectPath] ?? []), ...sessions];
			}
			return acc;
		}, {});
	}

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
	startSession(workspaceId: string, type: SessionType = 'claude') {
		this.workspaces.addAISession(workspaceId, type);
	}

	/** Start an AI session for a project (opens project if needed) */
	startSessionByProject(projectPath: string, type: SessionType = 'claude') {
		this.projects.openProject(projectPath);
		this.workspaces.addAIByProject(projectPath, type);
	}

	/** Start an AI session in a specific workspace */
	startSessionInWorkspace(
		ws: { id: string; projectPath: string; worktreePath?: string },
		type: SessionType = 'claude'
	) {
		this.startSession(ws.id, type);
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
		for (const ws of this.workspaces.workspaces) {
			for (const tab of ws.terminalTabs) {
				if (this.getAIPaneId(tab) === paneId && (tab.type === 'claude' || tab.type === 'codex')) {
					return tab.type;
				}
			}
		}
		return null;
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
			const existing = this.quiesceTimeouts.get(paneId);
			if (existing) clearTimeout(existing);
			this.quiesceTimeouts.set(
				paneId,
				setTimeout(() => {
					this.panesInProgress.delete(paneId);
					this.quiesceTimeouts.delete(paneId);
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

	private isIdlePromptNotification(payload: Record<string, unknown>): boolean {
		const notificationType = (
			this.payloadString(payload, 'notification_type') || this.payloadString(payload, 'type')
		).toLowerCase();
		if (notificationType === 'idle_prompt') return true;

		const message = this.payloadString(payload, 'message').toLowerCase();
		return message.includes('waiting for your input') || message.includes('waiting for input');
	}

	private onClaudeHookEvent(event: ClaudeHookEvent): void {
		const paneId = event.paneId;
		if (this.paneType(paneId) !== 'claude') return;

		if (event.sessionId) {
			this.workspaces.updateAISessionByPaneId(paneId, event.sessionId, 'claude');
			this.latestClaudeSessionByPane.set(paneId, event.sessionId);
			void this.syncClaudeLabelFromSession(paneId, event.sessionId);
		}

		switch (event.hookEventName) {
			case 'UserPromptSubmit':
				this.panesInProgress.add(paneId);
				break;
			case 'Stop':
			case 'SessionStart':
				this.panesInProgress.delete(paneId);
				break;
			case 'Notification':
				if (this.isIdlePromptNotification(event.hookPayload)) {
					this.panesInProgress.delete(paneId);
				}
				break;
			default:
				if (
					event.hookEventName?.startsWith('Notification:') &&
					this.isIdlePromptNotification(event.hookPayload)
				) {
					this.panesInProgress.delete(paneId);
				}
				break;
		}
	}

	private async syncClaudeLabelFromSession(paneId: string, sessionId: string): Promise<void> {
		const fallback = `Session ${sessionId.slice(0, 8)}`;
		this.workspaces.updateAITabLabelByPaneId(paneId, fallback, 'claude');

		const ctx = this.workspaces.findAIPaneContext(paneId, 'claude');
		if (!ctx) return;

		const sessions = await this.discoverSessions(ctx.projectPath);
		const latestSessionId = this.latestClaudeSessionByPane.get(paneId);
		if (latestSessionId !== sessionId) return;

		const match = sessions.find((s) => s.sessionId === sessionId);
		if (match?.label) {
			this.workspaces.updateAITabLabelByPaneId(paneId, match.label, 'claude');
		}
	}

	private onCodexNotifyEvent(event: CodexNotifyEvent): void {
		const paneId = event.paneId;
		if (this.paneType(paneId) !== 'codex') return;

		if (event.sessionId) {
			this.workspaces.updateAISessionByPaneId(paneId, event.sessionId, 'codex');
			this.latestCodexSessionByPane.set(paneId, event.sessionId);
			void this.syncCodexLabelFromSession(paneId, event.sessionId);
		}

		// Codex notify currently delivers completion/approval style events.
		if (event.notifyEvent === 'agent-turn-complete') {
			this.panesInProgress.delete(paneId);
		}
	}

	private async syncCodexLabelFromSession(paneId: string, sessionId: string): Promise<void> {
		const fallback = `Session ${sessionId.slice(0, 8)}`;
		this.workspaces.updateAITabLabelByPaneId(paneId, fallback, 'codex');

		const ctx = this.workspaces.findAIPaneContext(paneId, 'codex');
		if (!ctx) return;

		const sessions = await this.discoverCodexSessions(ctx.projectPath);
		const latestSessionId = this.latestCodexSessionByPane.get(paneId);
		if (latestSessionId !== sessionId) return;

		const match = sessions.find((s) => s.sessionId === sessionId);
		if (match?.label) {
			this.workspaces.updateAITabLabelByPaneId(paneId, match.label, 'codex');
		}
	}

	constructor(workspaces: WorkspaceStore, projects: ProjectStore) {
		this.workspaces = workspaces;
		this.projects = projects;

		listen<ClaudeHookEvent>('claude:hook', (event) => {
			this.onClaudeHookEvent(event.payload);
		});
		listen<CodexNotifyEvent>('codex:notify', (event) => {
			this.onCodexNotifyEvent(event.payload);
		});

		listen<TerminalDataEvent>('terminal:data', (event) => {
			const paneId = event.payload.sessionId;
			if (this.paneType(paneId) !== 'codex') return;
			if (this.classifyTerminalData(paneId, event.payload.data)) return;

			// Output received — mark as active, reset the debounce
			this.panesInProgress.add(paneId);
			const existing = this.quiesceTimeouts.get(paneId);
			if (existing) clearTimeout(existing);

			this.quiesceTimeouts.set(
				paneId,
				setTimeout(() => {
					this.panesInProgress.delete(paneId);
					this.quiesceTimeouts.delete(paneId);
				}, QUIET_THRESHOLD_MS)
			);
		});
	}
}
