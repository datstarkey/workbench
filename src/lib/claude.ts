/** Build the CLI command for a new Claude session */
export function claudeNewSessionCommand(sessionId: string): string {
	return `claude --session-id ${sessionId}`;
}

/** Build the CLI command to resume an existing Claude session */
export function claudeResumeCommand(sessionId: string): string {
	return `claude --resume ${sessionId}`;
}
