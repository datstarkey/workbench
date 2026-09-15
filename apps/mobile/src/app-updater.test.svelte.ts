import { describe, expect, it, vi } from 'vitest';
import { AppUpdater, type DownloadProgress, type UpdaterDeps } from './app-updater.svelte.ts';

const DOWNLOAD = 'https://github.com/datstarkey/workbench/releases/download/v0.29.0';
const UPDATE = {
	version: '0.29.0',
	apkUrl: `${DOWNLOAD}/workbench-android-0.29.0.apk`,
	sha256Url: `${DOWNLOAD}/workbench-android-0.29.0.apk.sha256`
};

const RELEASE = {
	tag_name: 'v0.29.0',
	draft: false,
	prerelease: false,
	assets: [
		{ name: 'workbench-android-0.29.0.apk', browser_download_url: UPDATE.apkUrl },
		{ name: 'workbench-android-0.29.0.apk.sha256', browser_download_url: UPDATE.sha256Url }
	]
};

function deps(overrides: Partial<UpdaterDeps> = {}): UpdaterDeps {
	return {
		isAndroid: () => true,
		getVersion: () => Promise.resolve('0.28.0'),
		fetch: vi.fn(() =>
			Promise.resolve(new Response(JSON.stringify(RELEASE)))
		) as unknown as typeof fetch,
		downloadAndInstall: vi.fn(() => Promise.resolve({ status: 'installing' as const })),
		...overrides
	};
}

async function available(overrides: Partial<UpdaterDeps> = {}) {
	const updater = new AppUpdater(deps(overrides));
	await updater.check();
	expect(updater.state.kind).toBe('available');
	return updater;
}

describe('AppUpdater', () => {
	it('does nothing off Android', async () => {
		const d = deps({ isAndroid: () => false });
		const updater = new AppUpdater(d);
		await updater.checkOnLaunch();
		expect(updater.supported).toBe(false);
		expect(updater.state.kind).toBe('idle');
		expect(d.fetch).not.toHaveBeenCalled();
	});

	it('offers a newer release and records the running version', async () => {
		const updater = await available();
		expect(updater.state).toEqual({ kind: 'available', update: UPDATE });
		expect(updater.version).toBe('0.28.0');
	});

	it('checks only once per launch', async () => {
		const d = deps();
		const updater = new AppUpdater(d);
		await updater.checkOnLaunch();
		await updater.checkOnLaunch();
		expect(d.fetch).toHaveBeenCalledTimes(1);
	});

	it('keeps a failed launch check silent but reports a manual one', async () => {
		const limited = vi.fn(() =>
			Promise.resolve(new Response('', { status: 403 }))
		) as unknown as typeof fetch;
		const updater = new AppUpdater(deps({ fetch: limited }));
		await updater.checkOnLaunch();
		expect(updater.state.kind).toBe('idle');
		await updater.check();
		expect(updater.state).toEqual({ kind: 'error', message: expect.stringMatching(/rate limit/) });
	});

	it('reports up to date only for a manual check', async () => {
		const current = deps({ getVersion: () => Promise.resolve('0.29.0') });
		const updater = new AppUpdater(current);
		await updater.checkOnLaunch();
		expect(updater.state.kind).toBe('idle');
		await updater.check();
		expect(updater.state.kind).toBe('up-to-date');
	});

	it('downloads with progress, then hands off to the installer', async () => {
		let report: ((p: DownloadProgress) => void) | undefined;
		let finish: (() => void) | undefined;
		const downloadAndInstall = vi.fn((_update, onProgress: (p: DownloadProgress) => void) => {
			report = onProgress;
			return new Promise<{ status: 'installing' }>((resolve) => {
				finish = () => resolve({ status: 'installing' });
			});
		});
		const updater = await available({ downloadAndInstall });

		const done = updater.install();
		expect(updater.state).toEqual({ kind: 'downloading', update: UPDATE, progress: null });
		expect(downloadAndInstall).toHaveBeenCalledWith(UPDATE, expect.any(Function));

		report?.({ downloaded: 25, total: 100 });
		expect(updater.state).toEqual({ kind: 'downloading', update: UPDATE, progress: 0.25 });
		report?.({ downloaded: 10, total: -1 });
		expect(updater.state).toEqual({ kind: 'downloading', update: UPDATE, progress: null });

		finish?.();
		await done;
		expect(updater.state).toEqual({ kind: 'installing', update: UPDATE });
	});

	it('ignores a second install while downloading', async () => {
		const downloadAndInstall = vi.fn(() => new Promise<never>(() => {}));
		const updater = await available({ downloadAndInstall });
		void updater.install();
		void updater.install();
		expect(downloadAndInstall).toHaveBeenCalledTimes(1);
	});

	it('surfaces the install-unknown-apps permission and retries', async () => {
		const downloadAndInstall = vi
			.fn()
			.mockResolvedValueOnce({ status: 'needs_permission' })
			.mockResolvedValueOnce({ status: 'installing' });
		const updater = await available({ downloadAndInstall });
		await updater.install();
		expect(updater.state).toEqual({ kind: 'needs-permission', update: UPDATE });
		await updater.install();
		expect(updater.state).toEqual({ kind: 'installing', update: UPDATE });
	});

	it('reports a native rejection and keeps the update for a retry', async () => {
		const downloadAndInstall = vi.fn(() =>
			Promise.reject({ message: 'Checksum mismatch, the download was discarded' })
		);
		const updater = await available({ downloadAndInstall });
		await updater.install();
		expect(updater.state).toEqual({
			kind: 'error',
			message: 'Checksum mismatch, the download was discarded',
			update: UPDATE
		});
	});

	it('dismiss() hides the banner', async () => {
		const updater = await available();
		updater.dismiss();
		expect(updater.state.kind).toBe('idle');
	});
});
