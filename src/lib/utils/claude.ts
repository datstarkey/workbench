const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** CLI command for a new Claude session (no session-id — let the CLI assign one) */
export const CLAUDE_NEW_SESSION_COMMAND = 'claude';

/** Build the CLI command to resume an existing Claude session */
export function claudeResumeCommand(sessionId: string): string {
	if (!UUID_RE.test(sessionId)) {
		throw new Error(`Invalid session ID: ${sessionId}`);
	}
	return `claude --resume ${sessionId}`;
}
