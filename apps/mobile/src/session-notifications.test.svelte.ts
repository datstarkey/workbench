import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { SessionNotifications, type NotificationApi } from './session-notifications.svelte';
import { stubLocalStorage } from './test-helpers';

beforeEach(stubLocalStorage);
afterEach(() => vi.unstubAllGlobals());
const connection = {
	url: 'http://mac',
	token: 'token',
	machineId: 'mac',
	name: 'Mac',
	sessionId: 'chat-1'
};
function api(): NotificationApi {
	return {
		start: vi.fn(async () => {}),
		stop: vi.fn(async () => {}),
		takeOpen: vi.fn(async () => ({ session: null })),
		listen: vi.fn(async () => () => {})
	};
}

it('is opt-in and reconfigures to the latest machine after an in-flight start', async () => {
	const native = api();
	const notifications = new SessionNotifications(native, true);
	await notifications.configure(connection);
	expect(native.start).not.toHaveBeenCalled();
	notifications.setEnabled(true);
	let release!: () => void;
	vi.mocked(native.start).mockImplementationOnce(
		() =>
			new Promise((r) => {
				release = r;
			})
	);
	const first = notifications.configure(connection);
	await Promise.resolve();
	const last = notifications.configure({ ...connection, machineId: 'pc', url: 'http://pc' });
	release();
	await first;
	await last;
	expect(vi.mocked(native.start).mock.calls.map(([c]) => c.machineId)).toEqual(['mac', 'pc']);
	await notifications.configure(null);
	expect(native.stop).toHaveBeenCalledTimes(2);
});

it('reports denied permission and disables the preference', async () => {
	const native = api();
	vi.mocked(native.start).mockRejectedValue(new Error('Permission denied'));
	const notifications = new SessionNotifications(native, true);
	notifications.setEnabled(true);
	await notifications.configure(connection);
	expect(notifications.enabled).toBe(false);
	expect(notifications.error).toBe('Permission denied');
});
