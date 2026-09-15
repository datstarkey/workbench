import { describe, expect, it, vi } from 'vitest';
import { LATEST_RELEASE_URL, checkForUpdate, parseSemver, selectUpdate } from './updater';

const DOWNLOAD = 'https://github.com/datstarkey/workbench/releases/download';

function release(tag: string, overrides: Record<string, unknown> = {}) {
	const version = tag.replace(/^v/, '');
	return {
		tag_name: tag,
		draft: false,
		prerelease: false,
		assets: [
			{ name: 'Workbench_universal.dmg', browser_download_url: `${DOWNLOAD}/${tag}/x.dmg` },
			{
				name: `workbench-android-${version}.apk`,
				browser_download_url: `${DOWNLOAD}/${tag}/workbench-android-${version}.apk`
			},
			{
				name: `workbench-android-${version}.apk.sha256`,
				browser_download_url: `${DOWNLOAD}/${tag}/workbench-android-${version}.apk.sha256`
			}
		],
		...overrides
	};
}

const fetchReturning = (res: Response) =>
	vi.fn(() => Promise.resolve(res)) as unknown as typeof fetch;

describe('parseSemver', () => {
	it('accepts plain and v-prefixed versions', () => {
		expect(parseSemver('0.28.0')).toEqual([0, 28, 0]);
		expect(parseSemver('v1.2.3')).toEqual([1, 2, 3]);
	});
	it('rejects malformed and pre-release versions', () => {
		expect(parseSemver('1.2')).toBeNull();
		expect(parseSemver('v1.2.3-beta.1')).toBeNull();
		expect(parseSemver('latest')).toBeNull();
	});
});

describe('selectUpdate', () => {
	it('offers a strictly newer release with both assets', () => {
		expect(selectUpdate(release('v0.29.0'), '0.28.0')).toEqual({
			kind: 'available',
			update: {
				version: '0.29.0',
				apkUrl: `${DOWNLOAD}/v0.29.0/workbench-android-0.29.0.apk`,
				sha256Url: `${DOWNLOAD}/v0.29.0/workbench-android-0.29.0.apk.sha256`
			}
		});
	});

	it('compares numerically, not lexically', () => {
		expect(selectUpdate(release('v0.10.0'), '0.9.9').kind).toBe('available');
		expect(selectUpdate(release('v0.9.10'), '0.10.0').kind).toBe('up-to-date');
	});

	it('ignores equal and older releases', () => {
		expect(selectUpdate(release('v0.28.0'), '0.28.0').kind).toBe('up-to-date');
		expect(selectUpdate(release('v0.27.2'), '0.28.0').kind).toBe('up-to-date');
	});

	it('ignores drafts and prereleases', () => {
		expect(selectUpdate(release('v1.0.0', { prerelease: true }), '0.28.0').kind).toBe('up-to-date');
		expect(selectUpdate(release('v1.0.0', { draft: true }), '0.28.0').kind).toBe('up-to-date');
	});

	it('ignores a malformed tag', () => {
		expect(selectUpdate(release('nightly'), '0.28.0').kind).toBe('up-to-date');
		expect(selectUpdate({ tag_name: 42 }, '0.28.0').kind).toBe('up-to-date');
	});

	it('is not an update when the APK or its checksum is missing', () => {
		const r = release('v0.29.0');
		const noApk = { ...r, assets: r.assets.filter((a) => !a.name.endsWith('.apk')) };
		const noSha = { ...r, assets: r.assets.filter((a) => !a.name.endsWith('.sha256')) };
		expect(selectUpdate(noApk, '0.28.0').kind).toBe('up-to-date');
		expect(selectUpdate(noSha, '0.28.0').kind).toBe('up-to-date');
		expect(selectUpdate({ ...r, assets: undefined }, '0.28.0').kind).toBe('up-to-date');
	});

	it('refuses asset URLs outside the Workbench releases', () => {
		const r = release('v0.29.0');
		r.assets[1].browser_download_url = 'https://evil.example/workbench-android-0.29.0.apk';
		expect(selectUpdate(r, '0.28.0').kind).toBe('up-to-date');
	});

	it('reports an unparseable running version', () => {
		expect(selectUpdate(release('v0.29.0'), 'dev').kind).toBe('error');
	});
});

describe('checkForUpdate', () => {
	it('fetches the latest release and selects the update', async () => {
		const fetchFn = fetchReturning(new Response(JSON.stringify(release('v0.29.0'))));
		const result = await checkForUpdate('0.28.0', fetchFn);
		expect(result.kind).toBe('available');
		expect(fetchFn).toHaveBeenCalledWith(LATEST_RELEASE_URL, expect.anything());
	});

	it('reports GitHub rate limiting', async () => {
		const result = await checkForUpdate(
			'0.28.0',
			fetchReturning(new Response('', { status: 403 }))
		);
		expect(result).toEqual({ kind: 'error', message: expect.stringMatching(/rate limit/) });
	});

	it('treats a repo without releases as up to date', async () => {
		const result = await checkForUpdate(
			'0.28.0',
			fetchReturning(new Response('', { status: 404 }))
		);
		expect(result.kind).toBe('up-to-date');
	});

	it('reports network failures and unexpected bodies', async () => {
		const offline = vi.fn(() => Promise.reject(new Error('offline'))) as unknown as typeof fetch;
		expect(await checkForUpdate('0.28.0', offline)).toEqual({ kind: 'error', message: 'offline' });
		const garbage = await checkForUpdate('0.28.0', fetchReturning(new Response('null')));
		expect(garbage.kind).toBe('error');
	});
});
