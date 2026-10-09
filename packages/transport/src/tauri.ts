import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ControlPlaneTransport, Capabilities } from './transport.ts';
import { workspaceMethods, type ServerAddress } from './workspace.ts';

interface LoopbackStatus {
	running: boolean;
	address: string | null;
	token: string | null;
}

let loopback: Promise<ServerAddress> | null = null;

/**
 * The desktop's always-on loopback listener (`terminal_server_status`), the
 * one every desktop-local request uses: the workspace model, terminal and
 * chat sockets. It keeps its port and token for the process lifetime, so the
 * lookup is memoized; a failure (asked before it was up) is retried next call.
 */
export function loopbackServer(): Promise<ServerAddress> {
	if (!loopback) {
		const lookup = (tauriInvoke('terminal_server_status') as Promise<LoopbackStatus>).then((s) => {
			if (!s.running || !s.address) throw new Error('embedded server is not running');
			return { baseUrl: `http://${s.address}`, token: s.token ?? undefined };
		});
		loopback = lookup;
		lookup.catch(() => {
			if (loopback === lookup) loopback = null;
		});
	}
	return loopback;
}

/** Test-only: forget the memoized loopback address. */
export function resetLoopbackServer(): void {
	loopback = null;
}

/**
 * Local transport for the desktop app — forwards control-plane commands to the
 * Rust backend over Tauri IPC.
 *
 * This is the ONLY transport that touches Tauri. Shared UI packages must never
 * import `@tauri-apps/*` directly — they go through this interface. `@tauri-apps/api`
 * is an optional peer dependency; consumers that never call
 * {@link createTauriTransport} (e.g. the mobile app) don't need it installed.
 */
export function createTauriTransport(): ControlPlaneTransport {
	const capabilities: Capabilities = { terminalIO: true, nativeDialogs: true };

	return {
		capabilities,
		// The workspace model lives in the server, so the desktop reaches it over
		// its loopback listener like any other client.
		...workspaceMethods(loopbackServer),

		invoke(name, args) {
			// Omit the args object entirely when absent so call shapes match what
			// the Rust IPC layer (and existing tests) expect.
			return (
				args === undefined
					? tauriInvoke(name as string)
					: tauriInvoke(name as string, args as Record<string, unknown>)
			) as never;
		},

		subscribe(event, cb) {
			return listen(event as string, (e) => cb(e.payload as never));
		}
	};
}
