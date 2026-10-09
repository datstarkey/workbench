import type { WorkbenchSettings } from './workbench.ts';

/** What the default `~/.claude` account is called. */
export function defaultAccountName(
	settings: Pick<WorkbenchSettings, 'defaultClaudeAccountName'> | null | undefined
): string {
	return settings?.defaultClaudeAccountName?.trim() || 'Default';
}
