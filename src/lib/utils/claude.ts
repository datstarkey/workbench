import type { SessionType } from '$types/workbench';

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** CLI command for a new Claude session (no session-id — let the CLI assign one) */
export const CLAUDE_NEW_SESSION_COMMAND = 'claude';

/** CLI command for a new Happy Coder session */
export const HAPPY_NEW_SESSION_COMMAND = 'happy';

/** CLI command for a new Codex session */
export const CODEX_NEW_SESSION_COMMAND = 'codex';

/** Return the base Claude/Happy binary name based on the useHappy flag */
function claudeBinary(useHappy?: boolean): string {
	return useHappy ? HAPPY_NEW_SESSION_COMMAND : CLAUDE_NEW_SESSION_COMMAND;
}

/** Build the CLI command to resume an existing Claude session */
export function claudeResumeCommand(sessionId: string, useHappy?: boolean): string {
	if (!UUID_RE.test(sessionId)) {
		throw new Error(`Invalid session ID: ${sessionId}`);
	}
	return `${claudeBinary(useHappy)} --resume ${sessionId}`;
}

/** Build the CLI command to resume an existing Codex session */
export function codexResumeCommand(sessionId: string): string {
	return `codex resume ${sessionId}`;
}

/** Generic helper: get the new-session command for a given session type */
export function newSessionCommand(type: SessionType, useHappy?: boolean): string {
	return type === 'codex' ? CODEX_NEW_SESSION_COMMAND : claudeBinary(useHappy);
}

/** Generic helper: get the resume command for a given session type */
export function resumeCommand(type: SessionType, sessionId: string, useHappy?: boolean): string {
	return type === 'codex'
		? codexResumeCommand(sessionId)
		: claudeResumeCommand(sessionId, useHappy);
}

/** Like resumeCommand but returns undefined for invalid session IDs instead of throwing. */
export function tryResumeCommand(
	type: SessionType,
	sessionId: string,
	useHappy?: boolean
): string | undefined {
	try {
		return resumeCommand(type, sessionId, useHappy);
	} catch {
		return undefined;
	}
}

const IS_WINDOWS = typeof navigator !== 'undefined' && navigator.userAgent.includes('Windows');

/** Quote a string for use in a shell command, handling platform differences. */
function shellQuote(value: string): string {
	if (IS_WINDOWS) {
		// cmd.exe / PowerShell: use double quotes with escaped inner quotes
		return `"${value.replaceAll('"', '\\"')}"`;
	}
	// Unix shells: single-quote with escaped embedded quotes
	return `'${value.replaceAll("'", "'\"'\"'")}'`;
}

function normalizePrompt(prompt: string): string {
	return prompt.replace(/\r\n/g, '\n').replace(/\r/g, '\n').trim();
}

/** Build an interactive new-session command that submits an initial prompt immediately. */
export function newSessionCommandWithPrompt(
	type: SessionType,
	prompt: string,
	useHappy?: boolean
): string {
	const normalizedPrompt = normalizePrompt(prompt);
	if (!normalizedPrompt) return newSessionCommand(type, useHappy);
	return `${newSessionCommand(type, useHappy)} ${shellQuote(normalizedPrompt)}`;
}
