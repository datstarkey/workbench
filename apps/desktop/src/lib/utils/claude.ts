import type { ClaudePermissionMode, CodexApprovalPolicy, CodexSandboxMode } from '$types/workbench';

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
