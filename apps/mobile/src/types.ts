export type TerminalMeta = {
	id: string;
	name?: string;
	cwd: string;
	createdAt: number;
	alive: boolean;
};

/** How a new Claude session opens on this phone. */
export type ClaudeView = 'chat' | 'terminal';

/** A Claude conversation, shown as chat or as a terminal running `claude`. */
export interface ChatRef {
	sessionId: string;
	projectPath: string;
	worktreePath?: string;
	name: string;
	/** The Claude account it belongs to; absent is the default login. */
	claudeAccountId?: string;
}

/** Extras for a terminal that runs `claude` on a conversation (the server builds the command). */
export interface ClaudeLaunch {
	claudeSession: { id: string; resume: boolean };
	claudeAccountId?: string;
}
