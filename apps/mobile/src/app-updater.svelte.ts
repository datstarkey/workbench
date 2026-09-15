import { getVersion } from '@tauri-apps/api/app';
import { Channel, invoke } from '@tauri-apps/api/core';
import { checkForUpdate, type AvailableUpdate } from './updater';

export type DownloadProgress = { downloaded: number; total: number };
export type InstallResult = { status: 'installing' | 'needs_permission' };

export type UpdaterState =
	| { kind: 'idle' }
	| { kind: 'checking' }
	| { kind: 'up-to-date' }
	| { kind: 'available'; update: AvailableUpdate }
	| { kind: 'downloading'; update: AvailableUpdate; progress: number | null }
	| { kind: 'installing'; update: AvailableUpdate }
	| { kind: 'needs-permission'; update: AvailableUpdate }
	| { kind: 'error'; message: string; update?: AvailableUpdate };

export type UpdaterDeps = {
	isAndroid: () => boolean;
	getVersion: () => Promise<string>;
	fetch: typeof fetch;
	downloadAndInstall: (
		update: AvailableUpdate,
		onProgress: (p: DownloadProgress) => void
	) => Promise<InstallResult>;
};

const defaultDeps: UpdaterDeps = {
	isAndroid: () => /Android/i.test(navigator.userAgent),
	getVersion,
	fetch: (...args) => fetch(...args),
	downloadAndInstall: (update, onProgress) => {
		const channel = new Channel<DownloadProgress>();
		channel.onmessage = onProgress;
		return invoke<InstallResult>('plugin:apk-updater|download_and_install', {
			url: update.apkUrl,
			sha256Url: update.sha256Url,
			onProgress: channel
		});
	}
};

/** Sideloaded-APK self-update: GitHub release check → download with progress → system installer. */
export class AppUpdater {
	state = $state.raw<UpdaterState>({ kind: 'idle' });
	version = $state<string | null>(null);

	readonly supported: boolean;
	private readonly deps: UpdaterDeps;
	private launchChecked = false;

	constructor(deps: UpdaterDeps = defaultDeps) {
		this.deps = deps;
		this.supported = deps.isAndroid();
	}

	/** Once per app launch; failures stay quiet so an offline start shows no banner. */
	async checkOnLaunch(): Promise<void> {
		if (this.launchChecked) return;
		this.launchChecked = true;
		await this.check(true);
	}

	async check(silent = false): Promise<void> {
		if (!this.supported || this.busy) return;
		this.state = { kind: 'checking' };
		try {
			this.version ??= await this.deps.getVersion();
			const result = await checkForUpdate(this.version, this.deps.fetch);
			if (result.kind === 'available') this.state = result;
			else if (result.kind === 'error' && !silent) this.state = result;
			else this.state = silent ? { kind: 'idle' } : { kind: 'up-to-date' };
		} catch (e) {
			this.state = silent ? { kind: 'idle' } : { kind: 'error', message: errorMessage(e) };
		}
	}

	async install(): Promise<void> {
		const update = 'update' in this.state ? this.state.update : undefined;
		if (!update || this.busy) return;
		this.state = { kind: 'downloading', update, progress: null };
		try {
			const result = await this.deps.downloadAndInstall(update, ({ downloaded, total }) => {
				if (this.state.kind !== 'downloading') return;
				const progress = total > 0 ? Math.min(1, downloaded / total) : null;
				this.state = { kind: 'downloading', update, progress };
			});
			this.state =
				result.status === 'needs_permission'
					? { kind: 'needs-permission', update }
					: { kind: 'installing', update };
		} catch (e) {
			this.state = { kind: 'error', message: errorMessage(e), update };
		}
	}

	dismiss(): void {
		if (!this.busy) this.state = { kind: 'idle' };
	}

	private get busy(): boolean {
		return this.state.kind === 'checking' || this.state.kind === 'downloading';
	}
}

// Native plugin rejections arrive as a `{ message }` object rather than an Error.
function errorMessage(e: unknown): string {
	if (typeof e === 'string') return e;
	if (e && typeof e === 'object' && 'message' in e && typeof e.message === 'string') {
		return e.message;
	}
	return String(e);
}
