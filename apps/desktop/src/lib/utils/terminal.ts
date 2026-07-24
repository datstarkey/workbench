import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { IntegrationStatus, TerminalExitEvent } from '$types/workbench';

// ── Exit event listener for native SwiftTerm terminals ─────────────────────
// The xterm path uses TerminalConnection (WS), so these listeners are only
// needed for NativeTerminalPane (PtyManager IPC path).

type ExitCallback = (payload: TerminalExitEvent) => void;

const exitSessionListeners = new Map<string, Set<ExitCallback>>();
let exitListenerReady: Promise<void> | null = null;

async function ensureExitListener(): Promise<void> {
	if (exitListenerReady) return exitListenerReady;
	exitListenerReady = listen<TerminalExitEvent>('terminal:exit', (event) => {
		const sessionListeners = exitSessionListeners.get(event.payload.sessionId);
		if (!sessionListeners) return;
		for (const cb of sessionListeners) cb(event.payload);
	}).then(() => undefined);
	return exitListenerReady;
}

export async function onSessionTerminalExit(
	sessionId: string,
	cb: (payload: TerminalExitEvent) => void
): Promise<UnlistenFn> {
	await ensureExitListener();
	let listeners = exitSessionListeners.get(sessionId);
	if (!listeners) {
		listeners = new Set<ExitCallback>();
		exitSessionListeners.set(sessionId, listeners);
	}
	listeners.add(cb);
	return () => {
		const current = exitSessionListeners.get(sessionId);
		if (!current) return;
		current.delete(cb);
		if (current.size === 0) exitSessionListeners.delete(sessionId);
	};
}

// ── Integration checks / apply ─────────────────────────────────────────────

export async function checkClaudeIntegration(): Promise<IntegrationStatus> {
	return invoke<IntegrationStatus>('check_claude_integration');
}

export async function checkCodexIntegration(): Promise<IntegrationStatus> {
	return invoke<IntegrationStatus>('check_codex_integration');
}

export async function applyClaudeIntegration(): Promise<boolean> {
	return invoke<boolean>('apply_claude_integration');
}

export async function applyCodexIntegration(): Promise<boolean> {
	return invoke<boolean>('apply_codex_integration');
}

// ── Native terminal (SwiftTerm) IPC wrappers ───────────────────────────────
// These are best-effort UI-sync calls. The backend rejects with
// "Session not found" once a session is cleaned up, but the UI may still fire
// a resize/visibility/write/kill during teardown. Swallow rejections so they
// don't surface as unhandled promise rejections.

function ignoreSessionGone(): void {}

export async function createNativeTerminal(request: {
	sessionId: string;
	projectPath: string;
	shell: string;
	x: number;
	y: number;
	width: number;
	height: number;
	fontSize: number;
	startupCommand?: string;
}): Promise<void> {
	await invoke('create_native_terminal', {
		sessionId: request.sessionId,
		projectPath: request.projectPath,
		shell: request.shell,
		x: request.x,
		y: request.y,
		width: request.width,
		height: request.height,
		fontSize: request.fontSize,
		startupCommand: request.startupCommand ?? null
	});
}

export async function resizeNativeTerminal(
	sessionId: string,
	x: number,
	y: number,
	width: number,
	height: number
): Promise<void> {
	await invoke('resize_native_terminal', { sessionId, x, y, width, height }).catch(
		ignoreSessionGone
	);
}

export async function setNativeTerminalVisible(sessionId: string, visible: boolean): Promise<void> {
	await invoke('set_native_terminal_visible', { sessionId, visible }).catch(ignoreSessionGone);
}

export async function killNativeTerminal(sessionId: string): Promise<void> {
	await invoke('kill_native_terminal', { sessionId }).catch(ignoreSessionGone);
}

export async function writeNativeTerminal(sessionId: string, data: string): Promise<void> {
	await invoke('write_native_terminal', { sessionId, data }).catch(ignoreSessionGone);
}

export async function isNativeTerminalAvailable(): Promise<boolean> {
	return invoke<boolean>('is_native_terminal_available');
}
