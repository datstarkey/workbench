import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { listen } from '@tauri-apps/api/event';

export type UpdateStatus = 'idle' | 'checking' | 'available' | 'downloading' | 'up-to-date' | 'error';

export class UpdaterStore {
	status = $state<UpdateStatus>('idle');
	progress = $state(0);
	contentLength = $state(0);
	error = $state<string | null>(null);
	version = $state<string | null>(null);
	body = $state<string | null>(null);
	dialogOpen = $state(false);

	private update: Update | null = null;

	constructor() {
		listen('menu:check-for-updates', () => {
			this.manualCheck();
		});

		// Auto-check after a short delay on startup
		setTimeout(() => this.checkForUpdates(false), 3000);
	}

	/** Manual check from menu — always opens dialog */
	async manualCheck() {
		this.dialogOpen = true;
		await this.checkForUpdates(true);
	}

	async checkForUpdates(showUpToDate: boolean) {
		this.status = 'checking';
		this.error = null;
		this.progress = 0;
		this.contentLength = 0;

		try {
			const update = await check();
			if (update) {
				this.update = update;
				this.version = update.version;
				this.body = update.body ?? null;
				this.status = 'available';
				this.dialogOpen = true;
			} else {
				this.update = null;
				this.status = 'up-to-date';
				if (!showUpToDate) {
					// Silent check — don't bother the user
					this.dialogOpen = false;
				}
			}
		} catch (e) {
			this.status = 'error';
			this.error = e instanceof Error ? e.message : String(e);
		}
	}

	async downloadAndInstall() {
		if (!this.update) return;

		this.status = 'downloading';
		this.progress = 0;

		try {
			await this.update.downloadAndInstall((event) => {
				if (event.event === 'Started') {
					this.contentLength = event.data.contentLength ?? 0;
				} else if (event.event === 'Progress') {
					this.progress += event.data.chunkLength;
				}
			});

			await relaunch();
		} catch (e) {
			this.status = 'error';
			this.error = e instanceof Error ? e.message : String(e);
		}
	}

	dismiss() {
		this.dialogOpen = false;
		if (this.status !== 'downloading') {
			this.status = 'idle';
		}
	}
}
