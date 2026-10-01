import { describe, it, expect, afterEach } from 'vitest';
import { invokeSpy, mockInvoke, clearInvokeMocks } from '../../../test/tauri-mocks';
import {
	ClaudeAuthStatuses,
	DEFAULT_ACCOUNT_KEY,
	describeAuth,
	suggestConfigDir
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

describe('ClaudeAuthStatuses', () => {
	afterEach(() => clearInvokeMocks());

	it('checks the default and every account, recording failures as null', async () => {
		mockInvoke('claude_auth_status', (args) => {
			const { accountId } = args as { accountId: string | null };
			if (accountId === 'broken') throw new Error('no claude');
			return { loggedIn: accountId === null };
		});
		const statuses = new ClaudeAuthStatuses();

		await statuses.refresh([
			{ id: 'work', name: 'Work', configDir: '/w' },
			{ id: 'broken', name: 'Broken', configDir: '/b' }
		]);

		expect(invokeSpy).toHaveBeenCalledWith('claude_auth_status', { accountId: null });
		expect(statuses.byKey[DEFAULT_ACCOUNT_KEY]).toEqual({ loggedIn: true });
		expect(statuses.byKey.work).toEqual({ loggedIn: false });
		expect(statuses.byKey.broken).toBeNull();
	});
});
