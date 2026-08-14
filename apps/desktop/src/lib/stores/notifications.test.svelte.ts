import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
	invokeSpy,
	mockInvoke,
	clearInvokeMocks,
	emitMockEvent,
	clearListeners
} from '../../test/tauri-mocks';

const sendNotification = vi.fn();
const isPermissionGranted = vi.fn(async () => true);
const requestPermission = vi.fn(async () => 'granted');
const onAction = vi.fn(async () => () => {});

vi.mock('@tauri-apps/plugin-notification', () => ({
	isPermissionGranted: (...a: unknown[]) => isPermissionGranted(...(a as [])),
	requestPermission: (...a: unknown[]) => requestPermission(...(a as [])),
	sendNotification: (...a: unknown[]) => sendNotification(...(a as [])),
	onAction: (...a: unknown[]) => onAction(...(a as []))
}));

const isFocused = vi.fn(async () => false);
const setFocus = vi.fn();
vi.mock('@tauri-apps/api/window', () => ({
	getCurrentWindow: () => ({
		isFocused: () => isFocused(),
		unminimize: vi.fn(),
		show: vi.fn(),
		setFocus: () => setFocus()
	})
}));

import { NotificationStore } from './notifications.svelte';
import type { ClaudeSessionStore } from './claudeSessions.svelte';
import type { WorkspaceStore } from './workspaces.svelte';

const PANE = 'pane-1';

function mockWorkspaces() {
	return {
		workspaces: [
			{
				id: 'ws-1',
				projectPath: '/test',
				projectName: 'Workbench',
				terminalTabs: [{ id: 'tab-1', label: 'Claude 1', panes: [{ id: PANE, type: 'claude' }] }],
				activeTerminalTabId: 'tab-1'
			}
		],
		activeWorkspace: null,
		focusPane: vi.fn()
	} as unknown as WorkspaceStore;
}

/** Captures the store's awaiting-input callback so tests can drive it directly. */
function mockSessions() {
	const holder: { fire?: (paneId: string) => void } = {};
	const sessions = {
		onAwaitingInput: (cb: (paneId: string) => void) => {
			holder.fire = cb;
		}
	} as unknown as ClaudeSessionStore;
	return { sessions, holder };
}

/** Route delivery through the native UNUserNotificationCenter bridge. */
function useNativePath(delivered = true) {
	mockInvoke('is_native_notification_available', () => true);
	mockInvoke('send_native_notification', () => delivered);
}

/** Let the store's async probe and notify chain settle. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('NotificationStore', () => {
	beforeEach(() => {
		sendNotification.mockClear();
		isPermissionGranted.mockReset().mockResolvedValue(true);
		requestPermission.mockReset().mockResolvedValue('granted');
		onAction.mockClear();
		isFocused.mockReset().mockResolvedValue(false);
		setFocus.mockClear();
	});

	afterEach(() => {
		clearInvokeMocks();
		clearListeners();
	});

	describe('native path (macOS)', () => {
		it('delivers through the native bridge, keyed by pane id', async () => {
			useNativePath();
			const { sessions, holder } = mockSessions();
			new NotificationStore(mockWorkspaces(), sessions);
			await settle();

			holder.fire?.(PANE);
			await settle();

			// The pane id doubles as the replace key, so a repeat notification for the
			// same pane replaces its banner instead of stacking.
			expect(invokeSpy).toHaveBeenCalledWith('send_native_notification', {
				identifier: PANE,
				title: 'Workbench — needs input',
				body: 'Claude 1 is waiting for your response'
			});
			// The plugin's macOS path posts to an API macOS no longer delivers on.
			expect(sendNotification).not.toHaveBeenCalled();
		});

		it('focuses the pane when its notification is clicked', async () => {
			useNativePath();
			const workspaces = mockWorkspaces();
			const { sessions } = mockSessions();
			new NotificationStore(workspaces, sessions);
			await settle();

			emitMockEvent('notification:action', PANE);
			await settle();

			expect(setFocus).toHaveBeenCalled();
			expect(workspaces.focusPane).toHaveBeenCalledWith(PANE);
		});

		it('warns rather than throwing when the user has denied authorization', async () => {
			useNativePath(false);
			const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

			const { sessions, holder } = mockSessions();
			new NotificationStore(mockWorkspaces(), sessions);
			await settle();

			holder.fire?.(PANE);
			await settle();

			expect(warn).toHaveBeenCalledWith(expect.stringContaining('not delivered'));
			warn.mockRestore();
		});

		it('does not notify before the availability probe resolves', async () => {
			let resolveProbe: (v: boolean) => void = () => {};
			mockInvoke(
				'is_native_notification_available',
				() =>
					new Promise<boolean>((resolve) => {
						resolveProbe = resolve;
					})
			);
			mockInvoke('send_native_notification', () => true);

			const { sessions, holder } = mockSessions();
			new NotificationStore(mockWorkspaces(), sessions);

			holder.fire?.(PANE);
			await settle();
			expect(invokeSpy).not.toHaveBeenCalledWith('send_native_notification', expect.anything());

			resolveProbe(true);
			await settle();

			expect(invokeSpy).toHaveBeenCalledWith('send_native_notification', expect.anything());
		});
	});

	describe('plugin path (non-macOS)', () => {
		// The native command isn't registered off macOS, so `invoke` rejects and the
		// store falls through to the plugin, which does deliver on Windows and Linux.
		beforeEach(() => {
			mockInvoke('is_native_notification_available', () => {
				throw new Error('command not found');
			});
		});

		it('uses the notification plugin when permission is granted', async () => {
			const { sessions, holder } = mockSessions();
			new NotificationStore(mockWorkspaces(), sessions);
			await settle();

			holder.fire?.(PANE);
			await settle();

			expect(sendNotification).toHaveBeenCalledWith(
				expect.objectContaining({
					title: 'Workbench — needs input',
					body: 'Claude 1 is waiting for your response'
				})
			);
		});

		it('respects an explicit permission denial', async () => {
			isPermissionGranted.mockResolvedValue(false);
			requestPermission.mockResolvedValue('denied');

			const { sessions, holder } = mockSessions();
			new NotificationStore(mockWorkspaces(), sessions);
			await settle();

			holder.fire?.(PANE);
			await settle();

			expect(sendNotification).not.toHaveBeenCalled();
		});

		it('stays quiet when the permission probe itself throws', async () => {
			isPermissionGranted.mockRejectedValue(new Error('unavailable'));
			const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

			const { sessions, holder } = mockSessions();
			new NotificationStore(mockWorkspaces(), sessions);
			await settle();

			holder.fire?.(PANE);
			await settle();

			expect(sendNotification).not.toHaveBeenCalled();
			warn.mockRestore();
		});
	});

	describe('suppression', () => {
		beforeEach(() => useNativePath());

		it('stays silent for a pane the user is already looking at', async () => {
			isFocused.mockResolvedValue(true);
			const workspaces = mockWorkspaces();
			(workspaces as { activeWorkspace: unknown }).activeWorkspace = workspaces.workspaces[0];

			const { sessions, holder } = mockSessions();
			new NotificationStore(workspaces, sessions);
			await settle();

			holder.fire?.(PANE);
			await settle();

			expect(invokeSpy).not.toHaveBeenCalledWith('send_native_notification', expect.anything());
		});

		it('still notifies for a background pane while the window is focused', async () => {
			isFocused.mockResolvedValue(true);
			const workspaces = mockWorkspaces();
			// Active tab holds a different pane, so PANE is not what the user is watching.
			(workspaces as { activeWorkspace: unknown }).activeWorkspace = {
				...workspaces.workspaces[0],
				terminalTabs: [{ id: 'tab-2', label: 'Other', panes: [{ id: 'pane-2', type: 'claude' }] }],
				activeTerminalTabId: 'tab-2'
			};

			const { sessions, holder } = mockSessions();
			new NotificationStore(workspaces, sessions);
			await settle();

			holder.fire?.(PANE);
			await settle();

			expect(invokeSpy).toHaveBeenCalledWith('send_native_notification', expect.anything());
		});
	});
});
