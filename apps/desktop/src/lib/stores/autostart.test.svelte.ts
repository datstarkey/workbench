import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart';
import { AutostartStore } from './autostart.svelte';

vi.mock('@tauri-apps/plugin-autostart', () => ({
	disable: vi.fn(),
	enable: vi.fn(),
	isEnabled: vi.fn()
}));

describe('AutostartStore', () => {
	beforeEach(() => {
		vi.resetAllMocks();
		vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue('Windows NT 10.0');
		vi.mocked(isEnabled).mockResolvedValue(false);
	});

	afterEach(() => vi.restoreAllMocks());

	it.each(['Windows NT 10.0', 'Macintosh; Intel Mac OS X 10_15_7'])(
		'reads the OS registration on %s without changing it',
		async (userAgent) => {
			vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(userAgent);
			vi.mocked(isEnabled).mockResolvedValue(true);
			const store = new AutostartStore();
			expect(store.disabled).toBe(true);
			await store.load();
			expect(store.enabled).toBe(true);
			expect(store.disabled).toBe(false);
			expect(enable).not.toHaveBeenCalled();
			expect(disable).not.toHaveBeenCalled();
		}
	);

	it('enables and disables launch at login', async () => {
		const store = new AutostartStore();
		await store.load();
		await store.setEnabled(true);
		expect(enable).toHaveBeenCalledOnce();
		expect(store.enabled).toBe(true);
		await store.setEnabled(false);
		expect(disable).toHaveBeenCalledOnce();
		expect(store.enabled).toBe(false);
	});

	it.each([true, false])('keeps the previous state when setting %s fails', async (value) => {
		vi.mocked(isEnabled).mockResolvedValue(!value);
		vi.mocked(value ? enable : disable).mockRejectedValueOnce('access denied');
		const store = new AutostartStore();
		await store.load();
		await store.setEnabled(value);
		expect(store.enabled).toBe(!value);
		expect(store.error).toContain('access denied');
		expect(store.busy).toBe(false);
		await store.setEnabled(value);
		expect(store.enabled).toBe(value);
		expect(store.error).toBe('');
	});

	it('allows a failed status read to be retried and blocks changes until it succeeds', async () => {
		vi.mocked(isEnabled).mockRejectedValueOnce('unavailable');
		const store = new AutostartStore();
		await store.load();
		expect(store.disabled).toBe(true);
		expect(store.error).toContain('unavailable');
		await store.setEnabled(true);
		expect(enable).not.toHaveBeenCalled();
		await store.load();
		expect(store.disabled).toBe(false);
		expect(store.error).toBe('');
	});

	it('ignores overlapping reads and toggles while changing the registration', async () => {
		let finish!: () => void;
		vi.mocked(enable).mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)));
		const store = new AutostartStore();
		await store.load();
		const pending = store.setEnabled(true);
		expect(store.disabled).toBe(true);
		await store.setEnabled(true);
		await store.setEnabled(false);
		await store.load();
		expect(enable).toHaveBeenCalledOnce();
		expect(disable).not.toHaveBeenCalled();
		expect(isEnabled).toHaveBeenCalledOnce();
		finish();
		await pending;
		expect(store.enabled).toBe(true);
		expect(store.disabled).toBe(false);
	});

	it('refreshes changes made outside Workbench', async () => {
		const store = new AutostartStore();
		await store.load();
		vi.mocked(isEnabled).mockResolvedValue(true);
		await store.load();
		expect(store.enabled).toBe(true);
	});

	it('does not call the native plugin on unsupported platforms', async () => {
		vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue('X11; Linux x86_64');
		const store = new AutostartStore();
		expect(store.supported).toBe(false);
		await store.load();
		await store.setEnabled(true);
		expect(isEnabled).not.toHaveBeenCalled();
		expect(enable).not.toHaveBeenCalled();
	});
});
