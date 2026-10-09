import type { AgentKind } from '@workbench/types';

/** How a new Claude session opens on this phone. */
export type ClaudeView = 'chat' | 'terminal';

/** A conversation a chat screen attaches to (never starts). */
export interface ChatRef {
	/** A pane shows its chat once it has one (a new Codex thread gets it from the server). */
	sessionId: string;
	/** Absent is Claude. */
	agent?: AgentKind;
	projectPath: string;
	worktreePath?: string;
	name: string;
	/** The Claude account it runs under; absent, the default login or (a new chat) the host decides. */
	claudeAccountId?: string;
}
