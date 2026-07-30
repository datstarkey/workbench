import {
	isPermissionGranted,
	requestPermission,
	sendNotification,
	onAction
} from '@tauri-apps/plugin-notification';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import { SvelteMap } from 'svelte/reactivity';
import type { ClaudeSessionStore } from './claudeSessions.svelte';
import type { WorkspaceStore } from './workspaces.svelte';

export class NotificationStore {
	private enabled = false;
	/** True only when the platform refused to tell us — the unsigned-build signature. */
	private permissionUnavailable = false;
	/** Awaited before notifying, so early events don't race the probe. */
	private permissionReady: Promise<void>;
	private workspaces: WorkspaceStore;
	/** Map notification id (numeric hash) → paneId for click routing. */
	private idToPane = new SvelteMap<number, string>();

	constructor(workspaces: WorkspaceStore, sessions: ClaudeSessionStore) {
		this.workspaces = workspaces;

		this.permissionReady = this.ensurePermission();

		onAction((notification) => {
			const id = typeof notification.id === 'number' ? notification.id : NaN;
			if (!Number.isFinite(id)) return;
			const paneId = this.idToPane.get(id);
			if (!paneId) return;
			void this.focusPane(paneId);
		}).catch((e) => {
			console.warn('[NotificationStore] Failed to register action handler:', e);
		});

		sessions.onAwaitingInput((paneId) => {
			this.notifyAwaitingInput(paneId);
		});
	}

	private async ensurePermission(): Promise<void> {
		try {
			let granted = await isPermissionGranted();
			if (!granted) {
				const result = await requestPermission();
				granted = result === 'granted';
			}
			this.enabled = granted;
		} catch (e) {
			// The plugin threw rather than answering — on macOS this is what an
			// ad-hoc-signed build looks like, because UNUserNotificationCenter won't
			// register it at all. A clean `denied` is a user decision and is respected.
			console.warn('[NotificationStore] Notification permission unavailable:', e);
			this.enabled = false;
			this.permissionUnavailable = true;
		}
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
		// Don't race the permission probe: an event during startup would otherwise take
		// the degraded fallback path even on a build where the plugin works.
		await this.permissionReady;
		// Suppress only when the user is actively looking at THIS pane —
		// other panes still get notified even when the window is focused.
		if ((await this.isWindowFocused()) && this.isPaneActive(paneId)) return;
		const ctx = this.findContext(paneId);
		if (!ctx) return;

		const title = `${ctx.projectName} — needs input`;
		const body = `${ctx.tabLabel} is waiting for your response`;

		if (this.enabled) {
			const notificationId = this.hashId(paneId);
			this.idToPane.set(notificationId, paneId);
			try {
				sendNotification({ id: notificationId, title, body });
				return;
			} catch (e) {
				console.warn('[NotificationStore] Failed to send notification:', e);
			}
		} else if (!this.permissionUnavailable) {
			// Permission was cleanly denied — that's the user's OS-level choice, and
			// routing around it via osascript would override them.
			return;
		}

		// An ad-hoc-signed build can't register with UNUserNotificationCenter, so the
		// plugin can't answer at all and every notification is silently dropped.
		// Fall back to osascript, which needs no code signing. This path is degraded:
		// no notification id (banners stack rather than replace) and no click-to-focus,
		// since the notification isn't ours to route. Both return with signing (#83).
		try {
			await invoke('send_fallback_notification', { title, body });
		} catch (e) {
			console.warn('[NotificationStore] Fallback notification failed:', e);
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
