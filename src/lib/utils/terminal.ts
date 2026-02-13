import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
	CreateTerminalRequest,
	CreateTerminalResponse,
	TerminalDataEvent,
	TerminalExitEvent
} from '$types/workbench';

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

export async function signalForeground(sessionId: string): Promise<boolean> {
	return invoke<boolean>('signal_foreground', { sessionId });
}

export async function killTerminal(sessionId: string): Promise<boolean> {
	return invoke<boolean>('kill_terminal', { sessionId });
}

export async function onTerminalData(
	cb: (payload: TerminalDataEvent) => void
): Promise<UnlistenFn> {
	return listen<TerminalDataEvent>('terminal:data', (event) => cb(event.payload));
}

export async function onTerminalExit(
	cb: (payload: TerminalExitEvent) => void
): Promise<UnlistenFn> {
	return listen<TerminalExitEvent>('terminal:exit', (event) => cb(event.payload));
}
