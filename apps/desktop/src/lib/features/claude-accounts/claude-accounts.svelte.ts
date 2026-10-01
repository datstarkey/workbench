// Desktop-local: logins live in this machine's keychain / config dirs.
import { invoke } from '@tauri-apps/api/core';
import type { ClaudeAccount, ClaudeAuthStatus, UsageLimit } from '$types/workbench';

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

/** Usage at or above this is flagged, so a near-limit account stands out. */
export const HIGH_USAGE_PERCENT = 80;

const WEEK_ALL_MODELS = 'week (all models)';

/** Compact plan usage, e.g. "Session 3% · Week 89%"; '' when there is none. */
export function describeUsage(limits: UsageLimit[] | null | undefined): string {
	if (!limits?.length) return '';
	const session = limits.find((l) => l.label === 'session');
	const week =
		limits.find((l) => l.label === WEEK_ALL_MODELS) ??
		limits.find((l) => l.label.startsWith('week'));
	return [session && `Session ${session.percent}%`, week && `Week ${week.percent}%`]
		.filter(Boolean)
		.join(' · ');
}

/** Every limit with its reset time, for a tooltip. */
export function usageDetail(limits: UsageLimit[] | null | undefined): string {
	return (limits ?? [])
		.map((l) => `${l.label}: ${l.percent}%${l.resets ? ` · resets ${l.resets}` : ''}`)
		.join('\n');
}

export function isHighUsage(limits: UsageLimit[] | null | undefined): boolean {
	return (limits ?? []).some((l) => l.percent >= HIGH_USAGE_PERCENT);
}

/**
 * Who each account is logged in as (`claude auth status`) and how much of its
 * plan limits it has used (`claude -p /usage`). Checked on demand (opening the
 * switcher) rather than polled: each check starts the CLI. `null` means the
 * check failed; usage is only checked for logged-in accounts.
 */
export class ClaudeAccountStatuses {
	authByKey = $state<Record<string, ClaudeAuthStatus | null>>({});
	usageByKey = $state<Record<string, UsageLimit[] | null>>({});

	async refresh(accounts: ClaudeAccount[]): Promise<void> {
		const ids = [undefined, ...accounts.map((a) => a.id)];
		await Promise.all(ids.map((id) => this.refreshOne(id)));
	}

	private async refreshOne(id: string | undefined): Promise<void> {
		const key = id ?? DEFAULT_ACCOUNT_KEY;
		const accountId = id ?? null;
		try {
			this.authByKey[key] = await invoke<ClaudeAuthStatus>('claude_auth_status', { accountId });
		} catch {
			this.authByKey[key] = null;
		}
		if (!this.authByKey[key]?.loggedIn) {
			delete this.usageByKey[key];
			return;
		}
		try {
			this.usageByKey[key] = await invoke<UsageLimit[]>('claude_usage', { accountId });
		} catch {
			this.usageByKey[key] = null;
		}
	}
}
