import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { SvelteSet } from 'svelte/reactivity';
import type { DiscoveredClaudeSession, TerminalDataEvent } from '$types/workbench';

const QUIET_THRESHOLD_MS = 1000;
const POLL_INTERVAL_MS = 2000;
const POLL_MAX_ATTEMPTS = 30;

class ClaudeSessionStore {
	/** Set of terminal pane IDs that are idle (no output for QUIET_THRESHOLD_MS) */
	panesNeedingAttention: SvelteSet<string> = $state(new SvelteSet());

	/** Cached discovered sessions for the current project */
	discoveredSessions: DiscoveredClaudeSession[] = $state([]);

	/** Per-pane debounce timeouts for quiescence detection */
	private quiesceTimeouts = new Map<string, ReturnType<typeof setTimeout>>();
	/** Active poll intervals keyed by tabId */
	private pollIntervals = new Map<string, ReturnType<typeof setInterval>>();
	/** Callback to check if a pane ID belongs to a Claude session */
	private isClaudePane: (paneId: string) => boolean = () => false;

	/** Read Claude CLI session files from ~/.claude/projects/ */
	async discoverSessions(projectPath: string): Promise<DiscoveredClaudeSession[]> {
		try {
			const sessions = await invoke<DiscoveredClaudeSession[]>('discover_claude_sessions', {
				projectPath
			});
			this.discoveredSessions = sessions;
			return sessions;
		} catch {
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

		const knownIds = new Set(this.discoveredSessions.map((s) => s.sessionId));
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

	/** Set the predicate used to decide which panes get quiescence tracking */
	setClaudePaneCheck(fn: (paneId: string) => boolean): void {
		this.isClaudePane = fn;
	}

	constructor() {
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

export const claudeSessionStore = new ClaudeSessionStore();
