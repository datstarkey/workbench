import { describe, it, expect, afterEach, vi } from 'vitest';
import { mockInvoke, clearInvokeMocks } from '../../../test/tauri-mocks';
import type { UsageLimit } from '$types/workbench';
import {
	ClaudeAccountStatuses,
	DEFAULT_ACCOUNT_KEY,
	describeAuth,
	describeUsage,
	isHighUsage,
	suggestConfigDir,
	usageDetail
} from './claude-accounts.svelte';

describe('suggestConfigDir', () => {
	it('slugs the name under the home dir', () => {
		expect(suggestConfigDir('/Users/jake', 'Work Account!')).toBe(
			'/Users/jake/.claude-work-account'
		);
		expect(suggestConfigDir('/Users/jake/', '  ')).toBe('/Users/jake/.claude-account');
	});

	it('keeps Windows separators', () => {
		expect(suggestConfigDir('C:\\Users\\jake', 'Work')).toBe('C:\\Users\\jake\\.claude-work');
	});
});

describe('describeAuth', () => {
	it('covers every state', () => {
		expect(describeAuth(undefined)).toBe('Checking…');
		expect(describeAuth(null)).toBe('Status unavailable');
		expect(describeAuth({ loggedIn: false })).toBe('Not logged in');
		expect(describeAuth({ loggedIn: true, email: 'a@b.c', subscriptionType: 'max' })).toBe(
			'a@b.c · max'
		);
	});
});

describe('usage formatting', () => {
	const limits: UsageLimit[] = [
		{ label: 'session', percent: 3, resets: 'Oct 1 at 5:10pm' },
		{ label: 'week (all models)', percent: 89, resets: 'Oct 2 at 9am' },
		{ label: 'week (Fable)', percent: 0 }
	];

	it('names limits like the chat chips', () => {
		expect(describeUsage(limits)).toBe('5h 3% · Week 89%');
		expect(describeUsage([{ label: 'week (Opus)', percent: 12 }])).toBe('Opus wk 12%');
		expect(describeUsage([])).toBe('');
		expect(describeUsage(null)).toBe('');
	});

	it('lists the shown limits with their resets for the tooltip', () => {
		expect(usageDetail(limits)).toBe(
			'5-hour session: 3% used · resets Oct 1 at 5:10pm\nWeekly limit: 89% used · resets Oct 2 at 9am'
		);
	});

	it('flags any limit at 80% or more', () => {
		expect(isHighUsage(limits)).toBe(true);
		expect(isHighUsage([{ label: 'session', percent: 79 }])).toBe(false);
		expect(isHighUsage(undefined)).toBe(false);
	});
});

describe('ClaudeAccountStatuses', () => {
	afterEach(() => clearInvokeMocks());

	it('checks login for every account and usage only for logged-in ones', async () => {
		mockInvoke('claude_auth_status', (args) => {
			const { accountId } = args as { accountId: string | null };
			if (accountId === 'broken') throw new Error('no claude');
			return { loggedIn: accountId !== 'out' };
		});
		const loadUsage = vi.fn(async (accountId?: string) => {
			if (accountId === 'work') throw new Error('timed out');
			return [{ label: 'session', percent: 3 }];
		});
		const statuses = new ClaudeAccountStatuses(loadUsage);

		await statuses.refresh([
			{ id: 'work', name: 'Work', configDir: '/w' },
			{ id: 'out', name: 'Out', configDir: '/o' },
			{ id: 'broken', name: 'Broken', configDir: '/b' }
		]);

		expect(statuses.authByKey[DEFAULT_ACCOUNT_KEY]).toEqual({ loggedIn: true });
		expect(statuses.usageByKey[DEFAULT_ACCOUNT_KEY]).toEqual([{ label: 'session', percent: 3 }]);
		expect(statuses.usageByKey.work).toBeNull();
		expect(statuses.authByKey.out).toEqual({ loggedIn: false });
		expect(statuses.authByKey.broken).toBeNull();
		expect(loadUsage.mock.calls.map(([id]) => id)).toEqual([undefined, 'work']);
	});
});
