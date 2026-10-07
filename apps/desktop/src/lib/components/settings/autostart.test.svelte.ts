import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { clearInvokeMocks, invokeSpy, mockInvoke } from '../../../test/tauri-mocks';
import { AutostartStore } from './autostart.svelte';

const platform = vi.hoisted(() => ({ IS_WINDOWS: true, IS_MAC: false }));
vi.mock('$lib/utils/platform', () => platform);

let osEnabled = false;
const setCalls = () => invokeSpy.mock.calls.filter(([cmd]) => cmd === 'set_autostart');
const readCalls = () => invokeSpy.mock.calls.filter(([cmd]) => cmd === 'autostart_enabled');

describe('AutostartStore', () => {
	beforeEach(() => {
		platform.IS_WINDOWS = true;
		platform.IS_MAC = false;
		osEnabled = false;
		mockInvoke('autostart_enabled', () => osEnabled);
		mockInvoke('set_autostart', (args) => {
			osEnabled = (args as { enabled: boolean }).enabled;
		});
	});

	afterEach(() => clearInvokeMocks());

	it.each([
		['Windows', true, false],
		['macOS', false, true]
	])('reads the OS registration on %s without changing it', async (_, windows, mac) => {
		platform.IS_WINDOWS = windows;
		platform.IS_MAC = mac;
		osEnabled = true;
		const store = new AutostartStore();
		expect(store.disabled).toBe(true);
		await store.load();
		expect(store.enabled).toBe(true);
		expect(store.disabled).toBe(false);
		expect(setCalls()).toHaveLength(0);
	});

	it('enables and disables launch at login', async () => {
		const store = new AutostartStore();
		await store.load();
		await store.setEnabled(true);
		expect(setCalls()).toEqual([['set_autostart', { enabled: true }]]);
		expect(store.enabled).toBe(true);
		await store.setEnabled(false);
		expect(setCalls()[1]).toEqual(['set_autostart', { enabled: false }]);
		expect(store.enabled).toBe(false);
	});

	it.each([true, false])('keeps the previous state when setting %s fails', async (value) => {
		osEnabled = !value;
		mockInvoke('set_autostart', () => {
			throw 'access denied';
		});
		const store = new AutostartStore();
		await store.load();
		await store.setEnabled(value);
		expect(store.enabled).toBe(!value);
		expect(store.error).toContain('access denied');
		expect(store.busy).toBe(false);
	});

	it('retries the failed change, not just the read', async () => {
		mockInvoke('set_autostart', () => {
			throw 'access denied';
		});
		const store = new AutostartStore();
		await store.load();
		await store.setEnabled(true);
		mockInvoke('set_autostart', () => {
			osEnabled = true;
		});
		await store.retry();
		expect(setCalls()).toHaveLength(2);
		expect(store.enabled).toBe(true);
		expect(store.error).toBe('');
	});

	it('keeps a failed change through a focus refresh', async () => {
		mockInvoke('set_autostart', () => {
			throw 'access denied';
		});
		const store = new AutostartStore();
		await store.load();
		await store.setEnabled(true);
		await store.refresh();
		expect(readCalls()).toHaveLength(1);
		expect(store.error).toContain('access denied');
	});

	it('retries a failed read and blocks changes until it succeeds', async () => {
		mockInvoke('autostart_enabled', () => {
			throw 'unavailable';
		});
		const store = new AutostartStore();
		await store.load();
		expect(store.disabled).toBe(true);
		expect(store.error).toContain('unavailable');
		await store.setEnabled(true);
		expect(setCalls()).toHaveLength(0);
		mockInvoke('autostart_enabled', () => osEnabled);
		await store.retry();
		expect(store.disabled).toBe(false);
		expect(store.error).toBe('');
	});

	it('ignores overlapping reads and toggles while changing the registration', async () => {
		let finish!: () => void;
		mockInvoke('set_autostart', () => new Promise<void>((resolve) => (finish = resolve)));
		const store = new AutostartStore();
		await store.load();
		const pending = store.setEnabled(true);
		expect(store.disabled).toBe(true);
		await store.setEnabled(true);
		await store.setEnabled(false);
		await store.load();
		expect(setCalls()).toHaveLength(1);
		expect(readCalls()).toHaveLength(1);
		finish();
		await pending;
		expect(store.enabled).toBe(true);
		expect(store.disabled).toBe(false);
	});

	it('refreshes changes made outside Workbench', async () => {
		const store = new AutostartStore();
		await store.load();
		osEnabled = true;
		await store.load();
		expect(store.enabled).toBe(true);
	});

	it('does not call the backend on unsupported platforms', async () => {
		platform.IS_WINDOWS = false;
		const store = new AutostartStore();
		expect(store.supported).toBe(false);
		await store.load();
		await store.setEnabled(true);
		expect(invokeSpy).not.toHaveBeenCalled();
	});
});
