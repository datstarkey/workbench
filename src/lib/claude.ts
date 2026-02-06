/** Build the CLI command for a new Claude session (no session-id — let the CLI assign one) */
export function claudeNewSessionCommand(): string {
	return 'claude';
}

/** Build the CLI command to resume an existing Claude session */
export function claudeResumeCommand(sessionId: string): string {
	return `claude --resume ${sessionId}`;
}
