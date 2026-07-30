import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invokeSpy, clearInvokeMocks } from '../../test/tauri-mocks';

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
vi.mock('@tauri-apps/api/window', () => ({
	getCurrentWindow: () => ({
		isFocused: () => isFocused(),
		unminimize: vi.fn(),
		show: vi.fn(),
		setFocus: vi.fn()
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

/** Let the store's async permission probe and notify chain settle. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('NotificationStore', () => {
	beforeEach(() => {
		sendNotification.mockClear();
		isPermissionGranted.mockReset().mockResolvedValue(true);
		requestPermission.mockReset().mockResolvedValue('granted');
		onAction.mockClear();
		isFocused.mockReset().mockResolvedValue(false);
	});

	afterEach(() => {
		clearInvokeMocks();
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
		expect(invokeSpy).not.toHaveBeenCalledWith('send_fallback_notification', expect.anything());
	});

	it('respects an explicit permission denial instead of routing around it', async () => {
		// A clean `denied` is the user's OS-level choice. Falling back to osascript here
		// would override them, which is different from the unsigned-build case below.
		isPermissionGranted.mockResolvedValue(false);
		requestPermission.mockResolvedValue('denied');

		const { sessions, holder } = mockSessions();
		new NotificationStore(mockWorkspaces(), sessions);
		await settle();

		holder.fire?.(PANE);
		await settle();

		expect(sendNotification).not.toHaveBeenCalled();
		expect(invokeSpy).not.toHaveBeenCalledWith('send_fallback_notification', expect.anything());
	});

	it('sends the right title and body on the fallback path', async () => {
		isPermissionGranted.mockRejectedValue(new Error('unavailable'));
		const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

		const { sessions, holder } = mockSessions();
		new NotificationStore(mockWorkspaces(), sessions);
		await settle();

		holder.fire?.(PANE);
		await settle();

		expect(invokeSpy).toHaveBeenCalledWith('send_fallback_notification', {
			title: 'Workbench — needs input',
			body: 'Claude 1 is waiting for your response'
		});
		warn.mockRestore();
	});

	it('does not take the degraded fallback while the permission probe is still pending', async () => {
		// An awaiting-input event during startup must wait for the probe, not assume
		// the plugin is unusable and lose click-to-focus on a build where it works.
		let resolveProbe: (v: boolean) => void = () => {};
		isPermissionGranted.mockReturnValue(
			new Promise<boolean>((resolve) => {
				resolveProbe = resolve;
			})
		);

		const { sessions, holder } = mockSessions();
		new NotificationStore(mockWorkspaces(), sessions);

		holder.fire?.(PANE);
		await settle();
		expect(invokeSpy).not.toHaveBeenCalledWith('send_fallback_notification', expect.anything());

		resolveProbe(true);
		await settle();

		expect(sendNotification).toHaveBeenCalled();
		expect(invokeSpy).not.toHaveBeenCalledWith('send_fallback_notification', expect.anything());
	});

	it('falls back when the permission probe itself throws', async () => {
		isPermissionGranted.mockRejectedValue(new Error('unavailable'));
		const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

		const { sessions, holder } = mockSessions();
		new NotificationStore(mockWorkspaces(), sessions);
		await settle();

		holder.fire?.(PANE);
		await settle();

		expect(invokeSpy).toHaveBeenCalledWith('send_fallback_notification', expect.anything());
		warn.mockRestore();
	});

	it('falls back when sendNotification throws despite permission', async () => {
		sendNotification.mockImplementation(() => {
			throw new Error('delivery failed');
		});
		const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

		const { sessions, holder } = mockSessions();
		new NotificationStore(mockWorkspaces(), sessions);
		await settle();

		holder.fire?.(PANE);
		await settle();

		expect(invokeSpy).toHaveBeenCalledWith('send_fallback_notification', expect.anything());
		warn.mockRestore();
		sendNotification.mockReset();
	});

	it('stays silent for a pane the user is already looking at', async () => {
		isFocused.mockResolvedValue(true);
		const workspaces = mockWorkspaces();
		(workspaces as { activeWorkspace: unknown }).activeWorkspace = workspaces.workspaces[0];

		const { sessions, holder } = mockSessions();
		new NotificationStore(workspaces, sessions);
		await settle();

		holder.fire?.(PANE);
		await settle();

		expect(sendNotification).not.toHaveBeenCalled();
		expect(invokeSpy).not.toHaveBeenCalledWith('send_fallback_notification', expect.anything());
	});

	it('still notifies for a background pane while the window is focused', async () => {
		isFocused.mockResolvedValue(true);
		const workspaces = mockWorkspaces();
		// Active tab holds a different pane, so PANE is not what the user is watching.
		(workspaces as { activeWorkspace: unknown }).activeWorkspace = {
			...workspaces.workspaces[0],
			terminalTabs: [{ id: 'tab-1', label: 'Claude 1', panes: [{ id: 'other-pane' }] }],
			activeTerminalTabId: 'tab-1'
		};

		const { sessions, holder } = mockSessions();
		new NotificationStore(workspaces, sessions);
		await settle();

		holder.fire?.(PANE);
		await settle();

		expect(sendNotification).toHaveBeenCalled();
	});
});
