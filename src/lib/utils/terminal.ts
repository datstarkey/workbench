import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
	CreateTerminalRequest,
	CreateTerminalResponse,
	IntegrationStatus,
	TerminalDataEvent,
	TerminalExitEvent
} from '$types/workbench';

type DataCallback = (payload: TerminalDataEvent) => void;
type ExitCallback = (payload: TerminalExitEvent) => void;

const dataSessionListeners = new Map<string, Set<DataCallback>>();
const exitSessionListeners = new Map<string, Set<ExitCallback>>();
const dataGlobalListeners = new Set<DataCallback>();
const exitGlobalListeners = new Set<ExitCallback>();

let dataListenerReady: Promise<void> | null = null;
let exitListenerReady: Promise<void> | null = null;

async function ensureDataListener(): Promise<void> {
	if (dataListenerReady) return dataListenerReady;
	dataListenerReady = listen<TerminalDataEvent>('terminal:data', (event) => {
		for (const cb of dataGlobalListeners) cb(event.payload);
		const sessionListeners = dataSessionListeners.get(event.payload.sessionId);
		if (!sessionListeners) return;
		for (const cb of sessionListeners) cb(event.payload);
	}).then(() => undefined);
	return dataListenerReady;
}

async function ensureExitListener(): Promise<void> {
	if (exitListenerReady) return exitListenerReady;
	exitListenerReady = listen<TerminalExitEvent>('terminal:exit', (event) => {
		for (const cb of exitGlobalListeners) cb(event.payload);
		const sessionListeners = exitSessionListeners.get(event.payload.sessionId);
		if (!sessionListeners) return;
		for (const cb of sessionListeners) cb(event.payload);
	}).then(() => undefined);
	return exitListenerReady;
}

export async function createTerminal(
	request: CreateTerminalRequest
): Promise<CreateTerminalResponse> {
	return invoke<CreateTerminalResponse>('create_terminal', { request });
}

// ── Input coalescing & serialization ────────────────────────────────
// Rapid keystrokes (especially Shift+letter for capitals) can fire multiple
// async invoke() calls simultaneously. Without ordering guarantees the PTY
// may receive keystrokes out-of-order, causing the shell line-editor to
// replay/garble text.
//
// Fix: per-session pending buffer + serial write chain.
//  • Regular chars accumulate until a microtask flushes them.
//  • Control chars (Enter, Ctrl+C, escapes, bracketed paste) flush immediately.
//  • A promise chain per session ensures writes reach the PTY in order.

const sessionInputBuffers = new Map<string, string>();
const sessionFlushScheduled = new Map<string, boolean>();
const sessionWriteChains = new Map<string, Promise<unknown>>();

function isControlInput(data: string): boolean {
	if (data.length === 0) return false;
	// Multi-char sequences (escape codes, bracketed paste) or control chars
	if (data.length > 1) return true;
	return data.charCodeAt(0) < 32;
}

function enqueueWrite(sessionId: string, data: string): void {
	const prev = sessionWriteChains.get(sessionId) ?? Promise.resolve();
	sessionWriteChains.set(
		sessionId,
		prev.then(
			() => invoke<boolean>('write_terminal', { sessionId, data }).catch(() => {}),
			() => invoke<boolean>('write_terminal', { sessionId, data }).catch(() => {})
		)
	);
}

function flushInputBuffer(sessionId: string): void {
	sessionFlushScheduled.delete(sessionId);
	const buffered = sessionInputBuffers.get(sessionId);
	if (!buffered) return;
	sessionInputBuffers.delete(sessionId);
	enqueueWrite(sessionId, buffered);
}

/** Clean up input coalescing state for a closed session. */
export function cleanupSessionInput(sessionId: string): void {
	sessionInputBuffers.delete(sessionId);
	sessionFlushScheduled.delete(sessionId);
	sessionWriteChains.delete(sessionId);
}

export function writeTerminal(sessionId: string, data: string): void {
	if (isControlInput(data)) {
		// Flush any pending regular chars first, then send control immediately
		const buffered = sessionInputBuffers.get(sessionId);
		if (buffered) {
			sessionInputBuffers.delete(sessionId);
			sessionFlushScheduled.delete(sessionId);
			enqueueWrite(sessionId, buffered);
		}
		enqueueWrite(sessionId, data);
		return;
	}

	// Coalesce regular chars and flush on next microtask
	const existing = sessionInputBuffers.get(sessionId) ?? '';
	sessionInputBuffers.set(sessionId, existing + data);

	if (!sessionFlushScheduled.get(sessionId)) {
		sessionFlushScheduled.set(sessionId, true);
		queueMicrotask(() => flushInputBuffer(sessionId));
	}
}

export async function resizeTerminal(
	sessionId: string,
	cols: number,
	rows: number
): Promise<boolean> {
	return invoke<boolean>('resize_terminal', { sessionId, cols, rows });
}

export async function killTerminal(sessionId: string): Promise<boolean> {
	return invoke<boolean>('kill_terminal', { sessionId });
}

export async function onTerminalData(
	cb: (payload: TerminalDataEvent) => void
): Promise<UnlistenFn> {
	await ensureDataListener();
	dataGlobalListeners.add(cb);
	return () => {
		dataGlobalListeners.delete(cb);
	};
}

export async function onSessionTerminalData(
	sessionId: string,
	cb: (payload: TerminalDataEvent) => void
): Promise<UnlistenFn> {
	await ensureDataListener();
	let listeners = dataSessionListeners.get(sessionId);
	if (!listeners) {
		listeners = new Set<DataCallback>();
		dataSessionListeners.set(sessionId, listeners);
	}
	listeners.add(cb);
	return () => {
		const current = dataSessionListeners.get(sessionId);
		if (!current) return;
		current.delete(cb);
		if (current.size === 0) dataSessionListeners.delete(sessionId);
	};
}

export async function onTerminalExit(
	cb: (payload: TerminalExitEvent) => void
): Promise<UnlistenFn> {
	await ensureExitListener();
	exitGlobalListeners.add(cb);
	return () => {
		exitGlobalListeners.delete(cb);
	};
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

// ── Native terminal (SwiftTerm) IPC wrappers ───────────────────────

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
	await invoke('resize_native_terminal', { sessionId, x, y, width, height });
}

export async function setNativeTerminalVisible(sessionId: string, visible: boolean): Promise<void> {
	await invoke('set_native_terminal_visible', { sessionId, visible });
}

export async function killNativeTerminal(sessionId: string): Promise<void> {
	await invoke('kill_native_terminal', { sessionId });
}

export async function writeNativeTerminal(sessionId: string, data: string): Promise<void> {
	await invoke('write_native_terminal', { sessionId, data });
}

export async function isNativeTerminalAvailable(): Promise<boolean> {
	return invoke<boolean>('is_native_terminal_available');
}
