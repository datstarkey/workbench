import { invoke } from '@tauri-apps/api/core';
import type { ClaudeSessionRecord, DiscoveredClaudeSession } from '$types/workbench';

interface ClaudeSessionsFile {
	sessions: ClaudeSessionRecord[];
}

class ClaudeSessionStore {
	sessions: ClaudeSessionRecord[] = $state([]);

	private persist() {
		const file: ClaudeSessionsFile = { sessions: this.sessions };
		invoke('save_claude_sessions', { file }).catch(() => {});
	}

	async load() {
		try {
			const file = await invoke<ClaudeSessionsFile>('load_claude_sessions');
			this.sessions = file.sessions;
		} catch {
			// No saved sessions
		}
	}

	addSession(record: ClaudeSessionRecord) {
		this.sessions = [...this.sessions, record];
		this.persist();
	}

	getByProject(projectPath: string): ClaudeSessionRecord[] {
		return this.sessions.filter((s) => s.projectPath === projectPath);
	}

	removeSession(id: string) {
		this.sessions = this.sessions.filter((s) => s.id !== id);
		this.persist();
	}

	/** Read Claude CLI session files from ~/.claude/projects/ */
	async discoverSessions(projectPath: string): Promise<DiscoveredClaudeSession[]> {
		try {
			return await invoke<DiscoveredClaudeSession[]>('discover_claude_sessions', {
				projectPath
			});
		} catch {
			return [];
		}
	}
}

export const claudeSessionStore = new ClaudeSessionStore();
