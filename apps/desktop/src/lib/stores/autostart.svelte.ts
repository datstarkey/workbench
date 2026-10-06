import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart';

/** The OS registration is the source of truth; changes take effect immediately. */
export class AutostartStore {
	readonly supported = /Windows|Macintosh|Mac OS X/.test(navigator.userAgent);
	enabled = $state<boolean | null>(null);
	busy = $state(false);
	error = $state('');
	disabled = $derived(this.busy || this.enabled === null);

	async load() {
		if (!this.supported || this.busy) return;
		this.busy = true;
		this.error = '';
		try {
			this.enabled = await isEnabled();
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
			if (value) await enable();
			else await disable();
			this.enabled = value;
		} catch (e) {
			this.error = `Couldn't change startup setting: ${String(e)}`;
		} finally {
			this.busy = false;
		}
	}
}
