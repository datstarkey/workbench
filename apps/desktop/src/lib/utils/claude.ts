import type {
	ClaudePermissionMode,
	ClaudeSessionLaunch,
	CodexApprovalPolicy,
	CodexSandboxMode,
	TerminalPaneState
} from '$types/workbench';
import { IS_WINDOWS } from './platform';

/**
 * What a Claude pane's terminal runs. The server (or, for a native terminal,
 * the desktop's Rust side) builds the command from it (`claude_launch`), so the
 * sandbox wrapper and permission mode can't be skipped; only Codex's command is
 * built here.
 */
export function claudeSessionLaunch(pane: TerminalPaneState): ClaudeSessionLaunch | undefined {
	if (pane.type !== 'claude' || !pane.claudeSessionId) return undefined;
	return { id: pane.claudeSessionId, ...(pane.claudePrompt && { prompt: pane.claudePrompt }) };
}

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** CLI command for a new Codex session */
export const CODEX_NEW_SESSION_COMMAND = 'codex';

/**
 * Run Codex inline rather than on the alternate screen, which has no scrollback:
 * xterm turns wheel (and mobile touch-scroll) input there into arrow keys, which
 * Codex reads as composer history. A `-c` key, not `--no-alt-screen`, because
 * older Codex builds ignore unknown config keys but refuse unknown flags.
 */
const CODEX_INLINE_FLAG = '-c tui.alternate_screen=never';
/** Every `-c` override Workbench writes, so a persisted command round-trips. */
const CODEX_OVERRIDES_RE =
	/^(?:-c[ \t]+(?:tui\.alternate_screen|approval_policy|sandbox_mode)=\S+[ \t]*)+/;

/** Values Codex accepts for `approval_policy`; 'default' writes no override. */
export const CODEX_APPROVAL_POLICIES: readonly CodexApprovalPolicy[] = [
	'default',
	'on-request',
	'never'
];

/** Values Codex accepts for `sandbox_mode`; 'default' writes no override. */
export const CODEX_SANDBOX_MODES: readonly CodexSandboxMode[] = [
	'default',
	'read-only',
	'workspace-write',
	'danger-full-access'
];

function isOneOf<T extends string>(values: readonly T[], value: unknown): value is T {
	return typeof value === 'string' && (values as readonly string[]).includes(value);
}

export const isCodexApprovalPolicy = (value: unknown): value is CodexApprovalPolicy =>
	isOneOf(CODEX_APPROVAL_POLICIES, value);

export const isCodexSandboxMode = (value: unknown): value is CodexSandboxMode =>
	isOneOf(CODEX_SANDBOX_MODES, value);

/** Modes the Claude CLI accepts for `--permission-mode` (the settings picker's list). */
export const CLAUDE_PERMISSION_MODES: readonly ClaudePermissionMode[] = [
	'default',
	'acceptEdits',
	'plan',
	'dontAsk',
	'auto',
	'bypassPermissions'
];

/** Narrow an untrusted value (settings JSON) to a known mode. */
export function isClaudePermissionMode(value: unknown): value is ClaudePermissionMode {
	return isOneOf(CLAUDE_PERMISSION_MODES, value);
}

/**
 * Render a `-c key=value` override, or '' for 'default' or an unknown value.
 * A `-c` key rather than `-a`/`-s` for the same reason as the inline flag, and
 * checked against the allowlist because settings JSON is user-editable and this
 * lands in a shell command.
 */
function codexOverride(key: string, value: string | undefined, allowed: readonly string[]) {
	if (!value || value === 'default' || !isOneOf(allowed, value)) return '';
	return ` -c ${key}=${value}`;
}

/** How Codex sessions should be launched. */
export interface LaunchOptions {
	/** Supported Workbench terminals own their process instead of a shared daemon. */
	codexNoDaemon?: boolean;
	codexApprovalPolicy?: CodexApprovalPolicy;
	codexSandboxMode?: CodexSandboxMode;
}

/** CLI command for a new Codex session. */
export function codexCommand(opts?: LaunchOptions): string {
	return (
		`${CODEX_NEW_SESSION_COMMAND}${opts?.codexNoDaemon ? ' --no-daemon' : ''} ${CODEX_INLINE_FLAG}` +
		codexOverride('approval_policy', opts?.codexApprovalPolicy, CODEX_APPROVAL_POLICIES) +
		codexOverride('sandbox_mode', opts?.codexSandboxMode, CODEX_SANDBOX_MODES)
	);
}

/** Build the CLI command to resume an existing Codex session */
export function codexResumeCommand(sessionId: string, opts?: LaunchOptions): string {
	if (!UUID_RE.test(sessionId)) {
		throw new Error(`Invalid session ID: ${sessionId}`);
	}
	return `${codexCommand(opts)} resume ${sessionId}`;
}

/** Like codexResumeCommand but returns undefined for invalid session IDs instead of throwing. */
export function tryCodexResumeCommand(sessionId: string, opts?: LaunchOptions): string | undefined {
	try {
		return codexResumeCommand(sessionId, opts);
	} catch {
		return undefined;
	}
}

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

/** A new Codex session that submits an initial prompt immediately. */
export function codexCommandWithPrompt(prompt: string, opts?: LaunchOptions): string {
	const normalizedPrompt = normalizePrompt(prompt);
	if (!normalizedPrompt) return codexCommand(opts);
	return `${codexCommand(opts)} ${shellQuote(normalizedPrompt)}`;
}

/**
 * Recover the initial-prompt argument from a persisted Codex launch command,
 * ignoring the binary and the `-c` overrides. Undefined when the command is not
 * a recognisable `codex` launch, so callers normalise it back to a fresh one.
 */
export function extractCodexPromptArg(command: string | undefined): string | undefined {
	const trimmed = command?.trim();
	if (!trimmed || !trimmed.startsWith(`${CODEX_NEW_SESSION_COMMAND} `)) return undefined;
	const rest = trimmed
		.slice(CODEX_NEW_SESSION_COMMAND.length + 1)
		.trimStart()
		.replace(/^--no-daemon[ \t]*/, '')
		.replace(CODEX_OVERRIDES_RE, '');
	return rest.length > 0 ? rest : undefined;
}
