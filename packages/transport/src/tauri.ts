import type { ControlPlaneTransport, Capabilities } from './transport.ts';

/**
 * Local transport for the desktop app — forwards control-plane commands to the
 * Rust backend over Tauri IPC. Uses dynamic imports so this package carries no
 * hard dependency on `@tauri-apps/api`; it is only loaded at runtime inside the
 * desktop app where Tauri is present.
 *
 * This is the ONLY transport that touches Tauri. Shared UI packages must never
 * import `@tauri-apps/*` directly — they go through this interface.
 */
export function createTauriTransport(): ControlPlaneTransport {
	const capabilities: Capabilities = { terminalIO: true, nativeDialogs: true };

	return {
		capabilities,

		async invoke(name, args) {
			const { invoke } = await import('@tauri-apps/api/core');
			return invoke(name as string, (args ?? undefined) as Record<string, unknown>) as never;
		},

		async subscribe(event, cb) {
			const { listen } = await import('@tauri-apps/api/event');
			const unlisten = await listen(event as string, (e) => cb(e.payload as never));
			return unlisten;
		}
	};
}
