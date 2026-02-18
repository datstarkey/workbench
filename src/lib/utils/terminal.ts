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

export async function writeTerminal(sessionId: string, data: string): Promise<boolean> {
	return invoke<boolean>('write_terminal', { sessionId, data });
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
