import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { HostUpdateStarted, HostUpdateStatus } from '$types/workbench';

export type UpdateStatus =
	| 'idle'
	| 'checking'
	| 'available'
	| 'downloading'
	/** Another device asked this host to install `version`; it restarts when done. */
	| 'remote'
	| 'up-to-date'
	| 'error';

/** `update:progress`, sent by Rust's one install path whoever started it. */
interface UpdateProgress {
	downloaded: number;
	total: number | null;
}

/** UI state for Rust's `host_update_*` commands, which install the same way a phone's `/host/update` does. */
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
		listen<string>('update:installing', ({ payload }) => {
			this.version = payload;
			// Ours already shows as downloading; any other install is another device's.
			if (this.status !== 'downloading') this.status = 'remote';
			this.dialogOpen = true;
		});
		listen<UpdateProgress>('update:progress', ({ payload }) => {
			this.progress = payload.downloaded;
			this.contentLength = payload.total ?? 0;
		});
		listen<string>('update:failed', ({ payload }) => {
			this.error =
				this.status === 'remote'
					? `The update started from another device failed: ${payload}`
					: payload;
			this.status = 'error';
			this.dialogOpen = true;
		});

		// Auto-check after a short delay on startup
		setTimeout(() => this.checkForUpdates(), 3000);
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
		this.progress = 0;
		this.contentLength = 0;

		try {
			const update = await invoke<HostUpdateStatus>('host_update_status');
			if (update.installing) {
				this.status = 'remote';
				this.dialogOpen = true;
			} else if (update.available) {
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

	/** Rust downloads, ends every session, installs and restarts; progress and failure come as events. */
	async downloadAndInstall() {
		if (this.status !== 'available') return;

		this.status = 'downloading';
		this.progress = 0;
		this.contentLength = 0;

		try {
			const started = await invoke<HostUpdateStarted>('host_update_install');
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
