import { invoke } from '@tauri-apps/api/core';
import { IS_MAC, IS_WINDOWS } from '$lib/utils/platform';

/** The OS login item is the source of truth; changes take effect immediately. */
export class AutostartStore {
	readonly supported = IS_WINDOWS || IS_MAC;
	enabled = $state<boolean | null>(null);
	busy = $state(false);
	error = $state('');
	disabled = $derived(this.busy || this.enabled === null);
	/** The change that last failed, so Retry repeats it rather than only re-reading. */
	private failedChange: boolean | null = null;

	async load() {
		if (!this.supported || this.busy) return;
		this.busy = true;
		this.error = '';
		this.failedChange = null;
		try {
			this.enabled = await invoke<boolean>('autostart_enabled');
		} catch (e) {
			this.enabled = null;
			this.error = `Couldn't read startup setting: ${String(e)}`;
		} finally {
			this.busy = false;
		}
	}

	async setEnabled(value: boolean) {
		if (!this.supported || this.disabled || value === this.enabled) return;
		this.busy = true;
		this.error = '';
		try {
			await invoke('set_autostart', { enabled: value });
			this.enabled = value;
			this.failedChange = null;
		} catch (e) {
			this.failedChange = value;
			this.error = `Couldn't change startup setting: ${String(e)}`;
		} finally {
			this.busy = false;
		}
	}

	retry() {
		return this.failedChange === null ? this.load() : this.setEnabled(this.failedChange);
	}

	/** Re-read after the window regains focus, keeping a failed change for Retry. */
	refresh() {
		return this.failedChange === null ? this.load() : Promise.resolve();
	}
}
