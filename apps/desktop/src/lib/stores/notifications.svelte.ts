import {
	isPermissionGranted,
	requestPermission,
	sendNotification,
	onAction
} from '@tauri-apps/plugin-notification';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { SvelteMap } from 'svelte/reactivity';
import type { ClaudeSessionStore } from './claudeSessions.svelte';
import type { WorkspaceStore } from './workspaces.svelte';

export class NotificationStore {
	/**
	 * Deliver through the native UNUserNotificationCenter bridge rather than the plugin.
	 * True on bundled macOS builds. The plugin's macOS path posts to
	 * NSUserNotificationCenter, which macOS stopped delivering after 10.15, and discards
	 * the error — so it fails silently and cannot be detected from here.
	 */
	private native = false;
	/** Plugin path only: whether permission was granted. */
	private enabled = false;
	/** Awaited before notifying, so early events don't race the probe. */
	private ready: Promise<void>;
	private workspaces: WorkspaceStore;
	/** Plugin path only: notification id (numeric hash) → paneId for click routing. */
	private idToPane = new SvelteMap<number, string>();

	constructor(workspaces: WorkspaceStore, sessions: ClaudeSessionStore) {
		this.workspaces = workspaces;
		this.ready = this.init();

		sessions.onAwaitingInput((paneId) => {
			this.notifyAwaitingInput(paneId);
		});
	}

	private async init(): Promise<void> {
		try {
			this.native = await invoke<boolean>('is_native_notification_available');
		} catch {
			// Command isn't registered off macOS, and is absent on unbundled dev builds.
			this.native = false;
		}

		if (this.native) {
			// The identifier is the paneId, so clicks route back without a lookup table.
			// Authorization is requested natively during setup(), before the app finishes
			// launching — doing it here would drop responses for early notifications.
			await listen<string>('notification:action', (event) => {
				void this.focusPane(event.payload);
			}).catch((e) => {
				console.warn('[NotificationStore] Failed to listen for actions:', e);
			});
			return;
		}

		try {
			let granted = await isPermissionGranted();
			if (!granted) {
				granted = (await requestPermission()) === 'granted';
			}
			this.enabled = granted;
		} catch (e) {
			console.warn('[NotificationStore] Notification permission unavailable:', e);
			this.enabled = false;
		}

		onAction((notification) => {
			const id = typeof notification.id === 'number' ? notification.id : NaN;
			if (!Number.isFinite(id)) return;
			const paneId = this.idToPane.get(id);
			if (!paneId) return;
			void this.focusPane(paneId);
		}).catch((e) => {
			console.warn('[NotificationStore] Failed to register action handler:', e);
		});
	}

	private findContext(paneId: string): {
		tabLabel: string;
		projectName: string;
	} | null {
		for (const ws of this.workspaces.workspaces) {
			for (const tab of ws.terminalTabs) {
				if (tab.panes.some((p) => p.id === paneId)) {
					return { tabLabel: tab.label, projectName: ws.projectName };
				}
			}
		}
		return null;
	}

	private async isWindowFocused(): Promise<boolean> {
		try {
			return await getCurrentWindow().isFocused();
		} catch {
			return false;
		}
	}

	private isPaneActive(paneId: string): boolean {
		const ws = this.workspaces.activeWorkspace;
		if (!ws) return false;
		const activeTab = ws.terminalTabs.find((t) => t.id === ws.activeTerminalTabId);
		if (!activeTab) return false;
		return activeTab.panes.some((p) => p.id === paneId);
	}

	private async notifyAwaitingInput(paneId: string): Promise<void> {
		await this.ready;
		// Suppress only when the user is actively looking at THIS pane —
		// other panes still get notified even when the window is focused.
		if ((await this.isWindowFocused()) && this.isPaneActive(paneId)) return;
		const ctx = this.findContext(paneId);
		if (!ctx) return;

		const title = `${ctx.projectName} — needs input`;
		const body = `${ctx.tabLabel} is waiting for your response`;

		if (this.native) {
			try {
				const delivered = await invoke<boolean>('send_native_notification', {
					identifier: paneId,
					title,
					body
				});
				// False means authorization was refused — the user's OS-level choice.
				if (!delivered) {
					console.warn('[NotificationStore] Notification not delivered (not authorized)');
				}
			} catch (e) {
				console.warn('[NotificationStore] Native notification failed:', e);
			}
			return;
		}

		if (!this.enabled) return;
		const notificationId = this.hashId(paneId);
		this.idToPane.set(notificationId, paneId);
		try {
			sendNotification({ id: notificationId, title, body });
		} catch (e) {
			console.warn('[NotificationStore] Failed to send notification:', e);
		}
	}

	private hashId(paneId: string): number {
		let hash = 0;
		for (let i = 0; i < paneId.length; i++) {
			hash = (hash * 31 + paneId.charCodeAt(i)) | 0;
		}
		return Math.abs(hash);
	}

	private async focusPane(paneId: string): Promise<void> {
		try {
			const win = getCurrentWindow();
			await win.unminimize();
			await win.show();
			await win.setFocus();
		} catch (e) {
			console.warn('[NotificationStore] Failed to focus window:', e);
		}
		this.workspaces.focusPane(paneId);
	}
}
