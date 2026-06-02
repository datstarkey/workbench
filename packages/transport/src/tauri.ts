import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ControlPlaneTransport, Capabilities } from './transport.ts';

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
