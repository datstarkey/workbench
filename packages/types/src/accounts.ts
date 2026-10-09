import type { ClaudeAccount, ProjectConfig, WorkbenchSettings } from './workbench.ts';

/** What the default `~/.claude` account is called. */
export function defaultAccountName(
	settings: Pick<WorkbenchSettings, 'defaultClaudeAccountName'> | null | undefined
): string {
	return settings?.defaultClaudeAccountName?.trim() || 'Default';
}

/**
 * The Claude account a new session in `project` starts under: the project's
 * own default (`''` is the default login, `~/.claude`), else `fallback` (the
 * active account). A project default naming a removed account falls back too.
 */
export function projectClaudeAccount(
	project: Pick<ProjectConfig, 'claudeAccountId'> | undefined,
	accounts: Pick<ClaudeAccount, 'id'>[],
	fallback: string | undefined
): string | undefined {
	const own = project?.claudeAccountId;
	if (own === '') return undefined;
	return own !== undefined && accounts.some((a) => a.id === own) ? own : fallback;
}
