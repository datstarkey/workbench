import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
	invokeSpy,
	mockInvoke,
	clearInvokeMocks,
	clearListeners,
	emitMockEvent
} from '../../test/tauri-mocks';
import type { HostUpdateStatus } from '$types/workbench';
import { UpdaterStore } from './updater.svelte';

const AVAILABLE: HostUpdateStatus = {
	current: '1.0.0',
	available: '9.9.9',
	body: 'Notes',
	installing: false
};

const installing = (startedBy: 'desktop' | 'remote'): HostUpdateStatus => ({
	current: '1.0.0',
	available: '9.9.9',
	installing: true,
	startedBy
});

afterEach(() => {
	vi.useRealTimers();
	clearInvokeMocks();
	clearListeners();
});

describe('UpdaterStore.checkForUpdates', () => {
	it('asks the feed afresh and opens the dialog on an update with its notes', async () => {
		mockInvoke('host_update_status', () => AVAILABLE);
		const store = new UpdaterStore();
		await store.checkForUpdates();
		expect(invokeSpy).toHaveBeenCalledWith('host_update_status', { fresh: true });
		expect(store.status).toBe('available');
		expect(store.version).toBe('9.9.9');
		expect(store.body).toBe('Notes');
		expect(store.dialogOpen).toBe(true);
	});

	it('shows a failed check', async () => {
		mockInvoke('host_update_status', () => Promise.reject('feed unreachable'));
		const store = new UpdaterStore();
		await store.manualCheck();
		expect(store.status).toBe('error');
		expect(store.error).toBe('feed unreachable');
	});

	it("shows another device's running install with its version", async () => {
		mockInvoke('host_update_status', () => installing('remote'));
		const store = new UpdaterStore();
		await store.checkForUpdates();
		expect(store.status).toBe('remote');
		expect(store.version).toBe('9.9.9');
		expect(store.busy).toBe(true);
	});

	it('shows its own install as downloading after a reload, keeping the progress', async () => {
		mockInvoke('host_update_status', () => installing('desktop'));
		const store = new UpdaterStore();
		emitMockEvent('update:progress', { downloaded: 50, total: 200 });
		await store.checkForUpdates();
		expect(store.status).toBe('downloading');
		expect(store.version).toBe('9.9.9');
		expect(store.progress).toBe(50);
	});

	it('skips the startup check once an install has shown up', async () => {
		vi.useFakeTimers();
		const store = new UpdaterStore();
		emitMockEvent('update:installing', { version: '9.9.9', origin: 'remote' });
		emitMockEvent('update:progress', { downloaded: 50, total: 200 });
		await vi.advanceTimersByTimeAsync(3000);
		expect(invokeSpy).not.toHaveBeenCalled();
		expect(store.status).toBe('remote');
		expect(store.progress).toBe(50);
	});

	it('keeps the dialog open when a silent check finishes during a manual one', async () => {
		vi.useFakeTimers();
		let finish!: () => void;
		mockInvoke(
			'host_update_status',
			() => new Promise((resolve) => (finish = () => resolve({ ...AVAILABLE, available: null })))
		);
		const store = new UpdaterStore();
		await vi.advanceTimersByTimeAsync(3000); // the startup check starts and hangs
		await store.manualCheck(); // already checking: opens the dialog, no second check
		expect(invokeSpy).toHaveBeenCalledTimes(1);
		finish();
		await vi.advanceTimersByTimeAsync(0);
		expect(store.dialogOpen).toBe(true);
		expect(store.status).toBe('up-to-date');
	});
});

describe('UpdaterStore.downloadAndInstall', () => {
	let store: UpdaterStore;

	beforeEach(async () => {
		mockInvoke('host_update_status', () => AVAILABLE);
		mockInvoke('host_update_install', () => {
			emitMockEvent('update:installing', { version: '9.9.9', origin: 'desktop' });
			return { version: '9.9.9' };
		});
		store = new UpdaterStore();
		await store.checkForUpdates();
	});

	it('installs the reviewed version through Rust and shows its progress', async () => {
		await store.downloadAndInstall();
		expect(invokeSpy).toHaveBeenCalledWith('host_update_install', { version: '9.9.9' });
		expect(store.status).toBe('downloading');
		expect(store.busy).toBe(true);

		emitMockEvent('update:progress', { downloaded: 50, total: 200 });
		expect(store.progress).toBe(50);
		expect(store.contentLength).toBe(200);
		emitMockEvent('update:progress', { downloaded: 200, total: 200 });
		expect(store.progress).toBe(200);
	});

	it('shows a failed install', async () => {
		await store.downloadAndInstall();
		emitMockEvent('update:failed', { error: 'offline', origin: 'desktop' });
		expect(store.status).toBe('error');
		expect(store.error).toBe('offline');
		expect(store.dialogOpen).toBe(true);
	});

	it('shows a refused install', async () => {
		mockInvoke('host_update_install', () =>
			Promise.reject('Workbench 10.0.0 is available now, not the version you reviewed')
		);
		await store.downloadAndInstall();
		expect(store.status).toBe('error');
		expect(store.error).toMatch(/10\.0\.0 is available now/);
	});

	it('refuses a second install while one runs', async () => {
		await store.downloadAndInstall();
		await store.downloadAndInstall();
		expect(invokeSpy.mock.calls.filter(([cmd]) => cmd === 'host_update_install')).toHaveLength(1);
	});

	it('shows an install started from another device until it fails', async () => {
		emitMockEvent('update:installing', { version: '9.9.9', origin: 'remote' });
		expect(store.status).toBe('remote');
		expect(store.busy).toBe(true);
		expect(store.dialogOpen).toBe(true);

		await store.downloadAndInstall();
		expect(invokeSpy).not.toHaveBeenCalledWith('host_update_install', expect.anything());

		emitMockEvent('update:failed', { error: 'offline', origin: 'remote' });
		expect(store.status).toBe('error');
		expect(store.error).toMatch(/another device failed: offline/);
	});
});
