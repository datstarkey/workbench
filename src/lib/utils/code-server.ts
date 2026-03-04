import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { CodeServerExitEvent, CodeServerInfo } from '$types/workbench';

export async function startCodeServer(
	sessionId: string,
	projectPath: string
): Promise<CodeServerInfo> {
	return invoke<CodeServerInfo>('start_code_server', { sessionId, projectPath });
}

export async function stopCodeServer(sessionId: string): Promise<boolean> {
	return invoke<boolean>('stop_code_server', { sessionId });
}

export async function detectCodeServer(): Promise<string> {
	return invoke<string>('detect_code_server');
}

// Singleton exit listener — one Tauri subscription dispatches to per-session callbacks
type ExitCallback = (payload: CodeServerExitEvent) => void;
const exitSessionListeners = new Map<string, Set<ExitCallback>>();
let exitListenerReady: Promise<void> | null = null;

function ensureExitListener(): Promise<void> {
	if (exitListenerReady) return exitListenerReady;
	exitListenerReady = listen<CodeServerExitEvent>('code-server:exit', (event) => {
		const cbs = exitSessionListeners.get(event.payload.sessionId);
		if (!cbs) return;
		for (const cb of cbs) cb(event.payload);
	}).then(() => undefined);
	return exitListenerReady;
}

export async function onCodeServerExit(sessionId: string, cb: ExitCallback): Promise<() => void> {
	await ensureExitListener();
	let cbs = exitSessionListeners.get(sessionId);
	if (!cbs) {
		cbs = new Set();
		exitSessionListeners.set(sessionId, cbs);
	}
	cbs.add(cb);
	return () => {
		const current = exitSessionListeners.get(sessionId);
		if (!current) return;
		current.delete(cb);
		if (current.size === 0) exitSessionListeners.delete(sessionId);
	};
}

/** Poll until code-server responds on its URL (or timeout). */
export async function waitForReady(url: string, timeoutMs = 15000): Promise<boolean> {
	const intervalMs = 500;
	const start = Date.now();
	while (Date.now() - start < timeoutMs) {
		try {
			// no-cors gives an opaque response, but lack of network error means server is up
			await fetch(url, { mode: 'no-cors' });
			return true;
		} catch {
			await new Promise((r) => setTimeout(r, intervalMs));
		}
	}
	return false;
}
