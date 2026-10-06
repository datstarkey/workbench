import type { AgentKind } from '@workbench/types';

export type TerminalMeta = {
	id: string;
	name?: string;
	cwd: string;
	createdAt: number;
	alive: boolean;
	/** The Claude conversation running in this terminal, including before its plugin attaches. */
	claudeSessionId?: string;
};

/** How a new Claude session opens on this phone. */
export type ClaudeView = 'chat' | 'terminal';

/**
 * A chat conversation: Claude (chat, or a terminal running `claude`) or Codex
 * (chat only).
 */
export interface ChatRef {
	/** Empty for a Codex chat not started yet: the server picks its thread id. */
	sessionId: string;
	/** Absent is Claude. */
	agent?: AgentKind;
	projectPath: string;
	worktreePath?: string;
	name: string;
	/** The Claude account it belongs to; absent is the default login. */
	claudeAccountId?: string;
	/** Join an existing process; only an explicit Restart may start it again. */
	attachOnly?: boolean;
}

/** Extras for a terminal that runs `claude` on a conversation (the server builds the command). */
export interface ClaudeLaunch {
	claudeSession: { id: string; resume: boolean };
	claudeAccountId?: string;
}
