import type { ClaudePermissionMode, SessionType } from '$types/workbench';

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** CLI command for a new Claude session (no session-id — let the CLI assign one) */
export const CLAUDE_NEW_SESSION_COMMAND = 'claude';

/** CLI command for a new Codex session */
export const CODEX_NEW_SESSION_COMMAND = 'codex';

/** How a Claude session should be launched. */
export interface ClaudeLaunchOptions {
	permissionMode?: ClaudePermissionMode;
	/**
	 * Absolute path to a generated `@anthropic-ai/sandbox-runtime` settings file.
	 * When set, the Claude launch is wrapped so file tools, MCP servers and hooks
	 * are all confined — not just Bash. Codex launches are never wrapped.
	 */
	sandboxSettingsPath?: string;
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

let warnedMissingSandboxPath = false;

/**
 * Warn once when the sandbox wrapper is switched on but the backend never
 * supplied a settings-file path, so the reason launches are unwrapped is visible
 * without spamming the console on every launch.
 */
export function warnMissingSandboxSettingsPath(): void {
	if (warnedMissingSandboxPath) return;
	warnedMissingSandboxPath = true;
	console.warn(
		'[claude] Sandbox runtime is enabled but no settings file path is available; launching Claude unwrapped.'
	);
}

/**
 * npm package providing the `srt` sandbox wrapper, pinned so an upstream release
 * cannot silently change the sandbox's semantics (or its argument parsing) under
 * a running install.
 *
 * NB the npm version and srt's own `--version` disagree: this package at 0.0.76
 * self-reports `0.2.0`. Pin the npm version — the other one does not resolve.
 */
export const SANDBOX_RUNTIME_PACKAGE = '@anthropic-ai/sandbox-runtime@0.0.76';

/** The same package without its version, for matching a prefix written by any build. */
const SANDBOX_RUNTIME_PACKAGE_NAME = '@anthropic-ai/sandbox-runtime';

/**
 * Render the sandbox-runtime wrapper prefix, or '' when it should not apply:
 * no settings file (the Rust side writes it and reports failure, in which case
 * the path is absent), or Windows, where srt is still alpha.
 *
 * The trailing `--` is load-bearing: srt parses its own options anywhere in the
 * argument list, so without the separator it swallows a wrapped command's
 * `--version`, `--debug`, `-s` or `-c` instead of passing them to `claude`.
 */
function sandboxPrefix(settingsPath: string | undefined): string {
	if (!settingsPath || IS_WINDOWS) return '';
	return `npx --yes ${SANDBOX_RUNTIME_PACKAGE} --settings ${shellQuote(settingsPath)} -- `;
}

/** Return the base Claude invocation, including any sandbox wrapper and permission-mode flag */
function claudeBinary(opts?: ClaudeLaunchOptions): string {
	return `${sandboxPrefix(opts?.sandboxSettingsPath)}${CLAUDE_NEW_SESSION_COMMAND}${permissionModeFlag(opts?.permissionMode)}`;
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

export const IS_WINDOWS =
	typeof navigator !== 'undefined' && navigator.userAgent.includes('Windows');

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
 * Matches a sandbox-runtime wrapper prefix: the fixed `npx --yes <pkg> --settings`
 * head, a settings path in any of the three forms `shellQuote` can produce
 * (single-quoted with `'"'"'` escapes, double-quoted, or bare), then the `--`
 * separator.
 */
const SANDBOX_PREFIX_RE = new RegExp(
	`^npx[ \\t]+--yes[ \\t]+${SANDBOX_RUNTIME_PACKAGE_NAME.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}` +
		// Any pinned version, or none, so a command written by an earlier build still strips.
		`(?:@[^\\s]+)?` +
		`[ \\t]+--settings[ \\t]+('(?:[^']|'"'"')*'|"(?:[^"\\\\]|\\\\.)*"|\\S+)[ \\t]+--[ \\t]+`
);

/**
 * Strip the sandbox-runtime wrapper from a persisted Claude command, so a
 * command written while the setting was on still round-trips after it is turned
 * off (and vice versa).
 */
function stripSandboxPrefix(command: string): string {
	return command.replace(SANDBOX_PREFIX_RE, '');
}

/**
 * Recover the initial-prompt argument from a persisted launch command, ignoring
 * the binary, any sandbox-runtime wrapper, and any `--permission-mode` flag
 * written by an earlier build. Returns undefined when the command is not a
 * recognisable launch of `type`'s binary, so callers normalise it back to a
 * freshly built command.
 */
export function extractPromptArg(
	type: SessionType,
	command: string | undefined
): string | undefined {
	let trimmed = command?.trim();
	const binary = type === 'codex' ? CODEX_NEW_SESSION_COMMAND : CLAUDE_NEW_SESSION_COMMAND;
	// Codex is never wrapped, so only unwrap on the Claude path.
	if (trimmed && type !== 'codex') trimmed = stripSandboxPrefix(trimmed);
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

/**
 * Rebuild an explicitly-supplied Claude launch command (a project startup
 * command, or a project task) so it carries the current sandbox wrapper and
 * permission mode, preserving whatever followed the binary.
 *
 * Without this, a project configured with a literal `claude` startup command
 * would launch unwrapped and unflagged, silently bypassing both settings.
 * Commands that are not a Claude launch are returned untouched.
 */
/** Flags that already decide permissions; adding `--permission-mode` would fight them. */
const PERMISSION_FLAG_RE =
	/(?:^|\s)--(?:permission-mode|dangerously-skip-permissions|allow-dangerously-skip-permissions)(?=\s|=|$)/;

export function applyClaudeLaunchOptions(command: string, opts?: ClaudeLaunchOptions): string {
	const trimmed = command.trim();
	const bare = stripSandboxPrefix(trimmed);
	const isClaudeLaunch =
		bare === CLAUDE_NEW_SESSION_COMMAND || bare.startsWith(`${CLAUDE_NEW_SESSION_COMMAND} `);
	if (!isClaudeLaunch) return trimmed;

	const args = bare.slice(CLAUDE_NEW_SESSION_COMMAND.length).trim();

	// The user already chose a permission posture on this command; the sandbox
	// wrapper is still applied, but the configured mode is not forced over it.
	const effectiveOpts = PERMISSION_FLAG_RE.test(args)
		? { sandboxSettingsPath: opts?.sandboxSettingsPath }
		: opts;

	const base = newSessionCommand('claude', effectiveOpts);

	// extractPromptArg drops the wrapper, the binary and any stale mode flag, so
	// a mode we *do* apply cannot end up duplicated.
	const rest = effectiveOpts === opts ? extractPromptArg('claude', trimmed) : args;

	// extractPromptArg returns undefined both for "no arguments" and for
	// "arguments I could not parse". Never silently drop a user's command: if
	// there were arguments, keep them verbatim.
	if (!rest) return args ? `${base} ${args}` : base;
	return `${base} ${rest}`;
}
