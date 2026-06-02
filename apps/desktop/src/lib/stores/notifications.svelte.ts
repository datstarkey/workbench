import {
	isPermissionGranted,
	requestPermission,
	sendNotification,
	onAction
} from '@tauri-apps/plugin-notification';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { SvelteMap } from 'svelte/reactivity';
import type { ClaudeSessionStore } from './claudeSessions.svelte';
import type { WorkspaceStore } from './workspaces.svelte';

export class NotificationStore {
	private enabled = false;
	private workspaces: WorkspaceStore;
	/** Map notification id (numeric hash) → paneId for click routing. */
	private idToPane = new SvelteMap<number, string>();

	constructor(workspaces: WorkspaceStore, sessions: ClaudeSessionStore) {
		this.workspaces = workspaces;

		void this.ensurePermission();

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
			console.warn('[NotificationStore] Failed to check permission:', e);
			this.enabled = false;
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
		if (!this.enabled) return;
		// Suppress only when the user is actively looking at THIS pane —
		// other panes still get notified even when the window is focused.
		if ((await this.isWindowFocused()) && this.isPaneActive(paneId)) return;
		const ctx = this.findContext(paneId);
		if (!ctx) return;
		const notificationId = this.hashId(paneId);
		this.idToPane.set(notificationId, paneId);
		try {
			sendNotification({
				id: notificationId,
				title: `${ctx.projectName} — needs input`,
				body: `${ctx.tabLabel} is waiting for your response`
			});
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
