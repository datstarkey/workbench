import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invokeSpy, mockInvoke, clearInvokeMocks, clearListeners } from '../../test/tauri-mocks';

const calls: string[] = [];
const update = {
	version: '9.9.9',
	body: null,
	download: vi.fn(async () => {
		calls.push('download');
	}),
	install: vi.fn(async () => {
		calls.push('install');
	})
};

vi.mock('@tauri-apps/plugin-updater', () => ({ check: vi.fn(async () => update) }));
vi.mock('@tauri-apps/plugin-process', () => ({
	relaunch: vi.fn(async () => {
		calls.push('relaunch');
	})
}));

import { check } from '@tauri-apps/plugin-updater';
import { UpdaterStore } from './updater.svelte';

describe('UpdaterStore.manualCheck', () => {
	afterEach(() => {
		vi.useRealTimers();
		vi.clearAllMocks();
		clearListeners();
	});

	it('keeps the dialog open when a silent check finishes during a manual one', async () => {
		vi.useFakeTimers();
		let finish!: () => void;
		vi.mocked(check).mockImplementationOnce(
			() => new Promise((resolve) => (finish = () => resolve(null)))
		);
		const store = new UpdaterStore();
		await vi.advanceTimersByTimeAsync(3000); // the startup check starts and hangs
		await store.manualCheck(); // already checking: opens the dialog, no second check
		expect(check).toHaveBeenCalledTimes(1);
		finish();
		await vi.advanceTimersByTimeAsync(0);
		expect(store.dialogOpen).toBe(true);
		expect(store.status).toBe('up-to-date');
	});
});

describe('UpdaterStore.downloadAndInstall', () => {
	let store: UpdaterStore;

	beforeEach(async () => {
		vi.useFakeTimers();
		calls.length = 0;
		mockInvoke('kill_all_sessions', () => calls.push('kill_all_sessions'));
		store = new UpdaterStore();
		await store.checkForUpdates();
	});

	afterEach(() => {
		vi.useRealTimers();
		vi.clearAllMocks();
		clearInvokeMocks();
		clearListeners();
	});

	it('kills every session after downloading and before installing', async () => {
		await store.downloadAndInstall();

		expect(calls).toEqual(['download', 'kill_all_sessions', 'install', 'relaunch']);
	});

	it('leaves sessions running when the download fails', async () => {
		update.download.mockRejectedValueOnce(new Error('offline'));

		await store.downloadAndInstall();

		expect(invokeSpy).not.toHaveBeenCalledWith('kill_all_sessions');
		expect(update.install).not.toHaveBeenCalled();
		expect(store.status).toBe('error');
		expect(store.error).toBe('offline');
	});
});
