import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { SvelteMap, SvelteSet } from 'svelte/reactivity';
import type {
	ActiveClaudeSession,
	DiscoveredClaudeSession,
	TerminalDataEvent
} from '$types/workbench';
import { effectivePath } from '$lib/utils/path';
import type { WorkspaceStore } from './workspaces.svelte';
import type { ProjectStore } from './projects.svelte';

const QUIET_THRESHOLD_MS = 1000;
const POLL_INTERVAL_MS = 2000;
const POLL_MAX_ATTEMPTS = 30;

export class ClaudeSessionStore {
	/** Set of terminal pane IDs that are idle (no output for QUIET_THRESHOLD_MS) */
	panesNeedingAttention: SvelteSet<string> = $state(new SvelteSet());

	/** Cached discovered sessions for the current project */
	discoveredSessions: DiscoveredClaudeSession[] = $state([]);

	/** Per-pane debounce timeouts for quiescence detection */
	private quiesceTimeouts = new SvelteMap<string, ReturnType<typeof setTimeout>>();
	/** Active poll intervals keyed by tabId */
	private pollIntervals = new SvelteMap<string, ReturnType<typeof setInterval>>();
	/** Reference to workspace store for Claude pane detection */
	private workspaces: WorkspaceStore;
	/** Reference to project store for opening projects */
	private projects: ProjectStore;

	/** Active Claude sessions grouped by project path */
	get activeSessionsByProject(): Record<string, ActiveClaudeSession[]> {
		return this.workspaces.workspaces.reduce<Record<string, ActiveClaudeSession[]>>((acc, ws) => {
			const sessions = ws.terminalTabs
				.filter((t) => t.type === 'claude')
				.map((t) => ({
					claudeSessionId: t.panes[0]?.claudeSessionId ?? '',
					tabId: t.id,
					label: t.label,
					needsAttention: this.panesNeedingAttention.has(t.panes[0]?.id ?? '')
				}));
			if (sessions.length > 0) {
				acc[ws.projectPath] = sessions;
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

	/** Remove a session from the discovered list (does not delete the JSONL file) */
	removeDiscoveredSession(sessionId: string): void {
		this.discoveredSessions = this.discoveredSessions.filter((s) => s.sessionId !== sessionId);
	}

	/**
	 * After launching a new Claude tab, poll discover_claude_sessions to find the
	 * session ID that Claude CLI assigned. Once found, invoke the onFound callback
	 * with the discovered session.
	 */
	pollForNewSession(
		tabId: string,
		projectPath: string,
		onFound: (session: DiscoveredClaudeSession) => void
	) {
		// Clear any existing poll for this tab
		const existing = this.pollIntervals.get(tabId);
		if (existing) clearInterval(existing);

		const knownIds = new SvelteSet(this.discoveredSessions.map((s) => s.sessionId));
		let attempts = 0;
		const interval = setInterval(async () => {
			attempts++;
			if (attempts > POLL_MAX_ATTEMPTS) {
				clearInterval(interval);
				this.pollIntervals.delete(tabId);
				return;
			}
			const sessions = await this.discoverSessions(projectPath);
			const newSession = sessions.find((s) => !knownIds.has(s.sessionId));
			if (newSession) {
				clearInterval(interval);
				this.pollIntervals.delete(tabId);
				onFound(newSession);
			}
		}, POLL_INTERVAL_MS);
		this.pollIntervals.set(tabId, interval);
	}

	/** Start a new Claude session: add tab + poll for session discovery */
	startSession(workspaceId: string, sessionPath: string) {
		const { tabId } = this.workspaces.addClaudeSession(workspaceId);
		this.pollForNewSession(tabId, sessionPath, (session) => {
			this.workspaces.updateClaudeTab(workspaceId, tabId, session.sessionId, session.label);
		});
	}

	/** Start a Claude session for a project (opens project if needed) */
	startSessionByProject(projectPath: string) {
		this.projects.openProject(projectPath);
		const result = this.workspaces.addClaudeByProject(projectPath);
		if (result) {
			this.pollForNewSession(result.tabId, projectPath, (session) => {
				this.workspaces.updateClaudeTab(
					result.workspaceId,
					result.tabId,
					session.sessionId,
					session.label
				);
			});
		}
	}

	/** Start a Claude session in a specific workspace */
	startSessionInWorkspace(ws: { id: string; projectPath: string; worktreePath?: string }) {
		const sessionPath = effectivePath(ws);
		this.startSession(ws.id, sessionPath);
	}

	private isClaudePane(paneId: string): boolean {
		return this.workspaces.workspaces.some((ws) =>
			ws.terminalTabs.some((t) => t.type === 'claude' && t.panes.some((p) => p.id === paneId))
		);
	}

	constructor(workspaces: WorkspaceStore, projects: ProjectStore) {
		this.workspaces = workspaces;
		this.projects = projects;

		listen<TerminalDataEvent>('terminal:data', (event) => {
			const paneId = event.payload.sessionId;
			if (!this.isClaudePane(paneId)) return;

			// Output received — mark as active, reset the debounce
			this.panesNeedingAttention.delete(paneId);
			const existing = this.quiesceTimeouts.get(paneId);
			if (existing) clearTimeout(existing);

			this.quiesceTimeouts.set(
				paneId,
				setTimeout(() => {
					this.panesNeedingAttention.add(paneId);
					this.quiesceTimeouts.delete(paneId);
				}, QUIET_THRESHOLD_MS)
			);
		});
	}
}
