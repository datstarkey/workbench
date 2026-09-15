export const LATEST_RELEASE_URL =
	'https://api.github.com/repos/datstarkey/workbench/releases/latest';
const RELEASE_DOWNLOAD_PREFIX = 'https://github.com/datstarkey/workbench/releases/download/';

type Semver = [number, number, number];

export type AvailableUpdate = { version: string; apkUrl: string; sha256Url: string };

export type UpdateCheck =
	| { kind: 'available'; update: AvailableUpdate }
	| { kind: 'up-to-date' }
	| { kind: 'error'; message: string };

type ReleaseAsset = { name?: unknown; browser_download_url?: unknown };
type Release = { tag_name?: unknown; draft?: unknown; prerelease?: unknown; assets?: unknown };

/** Parses a plain `x.y.z` (optionally `v`-prefixed). Pre-release/build suffixes are rejected. */
export function parseSemver(raw: string): Semver | null {
	const match = /^v?(\d+)\.(\d+)\.(\d+)$/.exec(raw.trim());
	return match ? [Number(match[1]), Number(match[2]), Number(match[3])] : null;
}

function isNewer(a: Semver, b: Semver): boolean {
	for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] > b[i];
	return false;
}

export function apkAssetName(version: string): string {
	return `workbench-android-${version}.apk`;
}

function assetUrl(assets: ReleaseAsset[], name: string): string | null {
	const url = assets.find((a) => a.name === name)?.browser_download_url;
	return typeof url === 'string' && url.startsWith(RELEASE_DOWNLOAD_PREFIX) ? url : null;
}

/** Picks the APK update out of a GitHub release, or explains why there isn't one. */
export function selectUpdate(release: Release, currentVersion: string): UpdateCheck {
	const current = parseSemver(currentVersion);
	if (!current) return { kind: 'error', message: `Unrecognised app version ${currentVersion}` };
	if (release.draft === true || release.prerelease === true) return { kind: 'up-to-date' };
	const latest = typeof release.tag_name === 'string' ? parseSemver(release.tag_name) : null;
	if (!latest || !isNewer(latest, current)) return { kind: 'up-to-date' };

	const version = latest.join('.');
	const assets = Array.isArray(release.assets) ? (release.assets as ReleaseAsset[]) : [];
	const apkUrl = assetUrl(assets, apkAssetName(version));
	const sha256Url = assetUrl(assets, `${apkAssetName(version)}.sha256`);
	// A release whose Android build hasn't been uploaded (yet) is not an update.
	if (!apkUrl || !sha256Url) return { kind: 'up-to-date' };
	return { kind: 'available', update: { version, apkUrl, sha256Url } };
}

export async function checkForUpdate(
	currentVersion: string,
	fetchFn: typeof fetch = fetch
): Promise<UpdateCheck> {
	let res: Response;
	try {
		res = await fetchFn(LATEST_RELEASE_URL, {
			headers: { accept: 'application/vnd.github+json' }
		});
	} catch (e) {
		return { kind: 'error', message: e instanceof Error ? e.message : String(e) };
	}
	if (res.status === 403 || res.status === 429) {
		return { kind: 'error', message: 'GitHub rate limit reached, try again later' };
	}
	if (res.status === 404) return { kind: 'up-to-date' };
	if (!res.ok) return { kind: 'error', message: `Update check failed (${res.status})` };
	try {
		return selectUpdate((await res.json()) as Release, currentVersion);
	} catch {
		return { kind: 'error', message: 'Unexpected response from GitHub' };
	}
}
