import { invoke } from '@tauri-apps/api/core';
import type { DiscoveredClaudeSession } from '$types/workbench';

class ClaudeSessionStore {
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
