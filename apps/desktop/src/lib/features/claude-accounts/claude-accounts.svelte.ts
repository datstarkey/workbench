// Desktop-local: logins live in this machine's keychain / config dirs.
import { invoke } from '@tauri-apps/api/core';
import type { ClaudeAccount, ClaudeAuthStatus } from '$types/workbench';

/** Status-map key for the implicit default `~/.claude` account. */
export const DEFAULT_ACCOUNT_KEY = '';

/** Run in a plain shell tab with the account's `CLAUDE_CONFIG_DIR` set. */
export const LOGIN_TASK = { name: 'Claude login', command: 'claude auth login' };

/** Suggested `CLAUDE_CONFIG_DIR` for a new account: `<home>/.claude-<slug>`. */
export function suggestConfigDir(home: string, name: string): string {
	const slug =
		name
			.trim()
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-+|-+$/g, '') || 'account';
	const sep = home.includes('\\') && !home.includes('/') ? '\\' : '/';
	return `${home.replace(/[\\/]+$/, '')}${sep}.claude-${slug}`;
}

/** One line for a login: "a@b.c · max", "Not logged in", … */
export function describeAuth(status: ClaudeAuthStatus | null | undefined): string {
	if (status === undefined) return 'Checking…';
	if (status === null) return 'Status unavailable';
	if (!status.loggedIn) return 'Not logged in';
	return [status.email, status.subscriptionType].filter(Boolean).join(' · ') || 'Logged in';
}

/**
 * Who each account is logged in as, from `claude auth status`. Checked on demand
 * (opening the switcher) rather than polled: each check starts the CLI.
 * `null` means the check failed.
 */
export class ClaudeAuthStatuses {
	byKey = $state<Record<string, ClaudeAuthStatus | null>>({});

	async refresh(accounts: ClaudeAccount[]): Promise<void> {
		const ids = [undefined, ...accounts.map((a) => a.id)];
		await Promise.all(
			ids.map(async (id) => {
				const key = id ?? DEFAULT_ACCOUNT_KEY;
				try {
					this.byKey[key] = await invoke<ClaudeAuthStatus>('claude_auth_status', {
						accountId: id ?? null
					});
				} catch {
					this.byKey[key] = null;
				}
			})
		);
	}
}
