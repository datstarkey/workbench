import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { HostUpdateStarted, HostUpdateStatus, UpdateOrigin } from '$types/workbench';

export type UpdateStatus =
	| 'idle'
	| 'checking'
	| 'available'
	| 'downloading'
	/** Another device asked this host to install `version`; it restarts when done. */
	| 'remote'
	| 'up-to-date'
	| 'error';

/** Events Rust's one install path sends, whoever started the install. */
interface UpdateInstalling {
	version: string;
	origin: UpdateOrigin;
}
interface UpdateProgress {
	downloaded: number;
	total: number | null;
}
interface UpdateFailed {
	error: string;
	origin: UpdateOrigin;
}

const installStatus = (origin: UpdateOrigin | null | undefined): UpdateStatus =>
	origin === 'desktop' ? 'downloading' : 'remote';

/** UI state for `host_update_status`/`host_update_install`: the contract a phone's `/host/update` has. */
export class UpdaterStore {
	status = $state<UpdateStatus>('idle');
	progress = $state(0);
	contentLength = $state(0);
	error = $state<string | null>(null);
	version = $state<string | null>(null);
	body = $state<string | null>(null);
	dialogOpen = $state(false);

	/** Installing: the dialog can't be dismissed. */
	get busy(): boolean {
		return this.status === 'downloading' || this.status === 'remote';
	}

	constructor() {
		listen('menu:check-for-updates', () => {
			this.manualCheck();
		});
		listen<UpdateInstalling>('update:installing', ({ payload }) => {
			this.version = payload.version;
			this.status = installStatus(payload.origin);
			this.dialogOpen = true;
		});
		listen<UpdateProgress>('update:progress', ({ payload }) => {
			this.progress = payload.downloaded;
			this.contentLength = payload.total ?? 0;
		});
		listen<UpdateFailed>('update:failed', ({ payload }) => {
			this.error =
				payload.origin === 'remote'
					? `The update started from another device failed: ${payload.error}`
					: payload.error;
			this.status = 'error';
			this.dialogOpen = true;
		});

		// Auto-check after a short delay on startup, unless an install already showed up.
		setTimeout(() => {
			if (this.status === 'idle') this.checkForUpdates();
		}, 3000);
	}

	/** Manual check from the menu or rail — always opens the dialog, which shows a check already running. */
	async manualCheck() {
		this.dialogOpen = true;
		if (this.status === 'checking' || this.busy) return;
		await this.checkForUpdates();
	}

	async checkForUpdates() {
		this.status = 'checking';
		this.error = null;

		try {
			const update = await invoke<HostUpdateStatus>('host_update_status', { fresh: true });
			if (update.installing) {
				// After a webview reload this can be our own install: keep the progress it reports.
				this.version = update.available;
				this.status = installStatus(update.startedBy);
				this.dialogOpen = true;
				return;
			}
			this.progress = 0;
			this.contentLength = 0;
			if (update.available) {
				this.version = update.available;
				this.body = update.body ?? null;
				this.status = 'available';
				this.dialogOpen = true;
			} else {
				this.status = 'up-to-date';
			}
		} catch (e) {
			this.status = 'error';
			this.error = e instanceof Error ? e.message : String(e);
		}
	}

	/** Installs the version the dialog showed; Rust refuses if the feed now offers another. */
	async downloadAndInstall() {
		if (this.status !== 'available' || !this.version) return;

		this.status = 'downloading';
		this.progress = 0;
		this.contentLength = 0;

		try {
			const started = await invoke<HostUpdateStarted>('host_update_install', {
				version: this.version
			});
			this.version = started.version;
		} catch (e) {
			this.status = 'error';
			this.error = e instanceof Error ? e.message : String(e);
		}
	}

	dismiss() {
		this.dialogOpen = false;
		if (!this.busy) {
			this.status = 'idle';
		}
	}
}
