import type { ClaudePermissionMode, SessionType } from '$types/workbench';

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** CLI command for a new Claude session (no session-id — let the CLI assign one) */
export const CLAUDE_NEW_SESSION_COMMAND = 'claude';

/** CLI command for a new Codex session */
export const CODEX_NEW_SESSION_COMMAND = 'codex';

/** How a Claude session should be launched. */
export interface ClaudeLaunchOptions {
	permissionMode?: ClaudePermissionMode;
}

/** Modes the Claude CLI accepts for `--permission-mode`. */
export const CLAUDE_PERMISSION_MODES: readonly ClaudePermissionMode[] = [
	'default',
	'acceptEdits',
	'plan',
	'dontAsk',
	'auto',
	'bypassPermissions'
];

/** Narrow an untrusted value (settings JSON, a persisted command) to a known mode. */
export function isClaudePermissionMode(value: unknown): value is ClaudePermissionMode {
	return (
		typeof value === 'string' && (CLAUDE_PERMISSION_MODES as readonly string[]).includes(value)
	);
}

/**
 * Render the `--permission-mode` flag, or '' when the mode is absent, 'default',
 * or unrecognised. Settings JSON is user-editable on disk, so the value is
 * checked against the allowlist before being interpolated into a shell command.
 */
function permissionModeFlag(mode: ClaudePermissionMode | undefined): string {
	if (!mode || mode === 'default' || !isClaudePermissionMode(mode)) return '';
	return ` --permission-mode ${mode}`;
}

/** Return the base Claude invocation, including any permission-mode flag */
function claudeBinary(opts?: ClaudeLaunchOptions): string {
	return `${CLAUDE_NEW_SESSION_COMMAND}${permissionModeFlag(opts?.permissionMode)}`;
}

/** Build the CLI command to resume an existing Claude session */
export function claudeResumeCommand(sessionId: string, opts?: ClaudeLaunchOptions): string {
	if (!UUID_RE.test(sessionId)) {
		throw new Error(`Invalid session ID: ${sessionId}`);
	}
	return `${claudeBinary(opts)} --resume ${sessionId}`;
}

/** Build the CLI command to resume an existing Codex session */
export function codexResumeCommand(sessionId: string): string {
	return `codex resume ${sessionId}`;
}

/** Generic helper: get the new-session command for a given session type */
export function newSessionCommand(type: SessionType, opts?: ClaudeLaunchOptions): string {
	return type === 'codex' ? CODEX_NEW_SESSION_COMMAND : claudeBinary(opts);
}

/** Generic helper: get the resume command for a given session type */
export function resumeCommand(
	type: SessionType,
	sessionId: string,
	opts?: ClaudeLaunchOptions
): string {
	return type === 'codex' ? codexResumeCommand(sessionId) : claudeResumeCommand(sessionId, opts);
}

/** Like resumeCommand but returns undefined for invalid session IDs instead of throwing. */
export function tryResumeCommand(
	type: SessionType,
	sessionId: string,
	opts?: ClaudeLaunchOptions
): string | undefined {
	try {
		return resumeCommand(type, sessionId, opts);
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
	opts?: ClaudeLaunchOptions
): string {
	const normalizedPrompt = normalizePrompt(prompt);
	if (!normalizedPrompt) return newSessionCommand(type, opts);
	return `${newSessionCommand(type, opts)} ${shellQuote(normalizedPrompt)}`;
}

/**
 * Recover the initial-prompt argument from a persisted launch command, ignoring
 * the binary and any `--permission-mode` flag written by an earlier build. Returns
 * undefined when the command is not a recognisable launch of `type`'s binary, so
 * callers normalise it back to a freshly built command.
 */
export function extractPromptArg(
	type: SessionType,
	command: string | undefined
): string | undefined {
	const trimmed = command?.trim();
	const binary = type === 'codex' ? CODEX_NEW_SESSION_COMMAND : CLAUDE_NEW_SESSION_COMMAND;
	if (!trimmed || !trimmed.startsWith(`${binary} `)) return undefined;

	let rest = trimmed.slice(binary.length + 1).trimStart();
	if (type !== 'codex') {
		const flag = /^--permission-mode[ \t]+(\S+)[ \t]*/.exec(rest);
		// An unknown mode means we did not write this command; normalise it away.
		if (flag && !isClaudePermissionMode(flag[1])) return undefined;
		if (flag) rest = rest.slice(flag[0].length);
	}
	return rest.length > 0 ? rest : undefined;
}
