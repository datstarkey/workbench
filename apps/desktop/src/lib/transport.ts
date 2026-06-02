/**
 * Desktop transport holder.
 *
 * The desktop app talks to its own Rust backend through {@link createTauriTransport}
 * by default. In "remote" mode (connecting to a headless `workbench-server`) the
 * app swaps in {@link createHttpTransport} before stores load, and the
 * control-plane stores transparently issue HTTP requests instead of Tauri IPC.
 *
 * Control-plane stores import `invoke`/`listen` from THIS module instead of from
 * `@tauri-apps/api` — the signatures match Tauri's, so call sites are unchanged,
 * but the calls are routed through the active {@link ControlPlaneTransport}.
 * Terminal IO and other desktop-only Tauri calls keep importing `@tauri-apps/api`
 * directly — they are local-only and never cross to a remote server.
 */
import {
	createHttpTransport,
	createTauriTransport,
	type ControlPlaneEvents,
	type ControlPlaneTransport,
	type HttpTransportOptions,
	type Unsubscribe
} from '@workbench/transport';

let current: ControlPlaneTransport = createTauriTransport();

/** Replace the active transport. Call before stores load. */
export function setTransport(transport: ControlPlaneTransport) {
	current = transport;
}

/** Switch the app into remote mode against a workbench-server. */
export function useRemoteTransport(opts: HttpTransportOptions) {
	current = createHttpTransport(opts);
}

/** The active transport (for capability checks, e.g. `transport().capabilities.terminalIO`). */
export function transport(): ControlPlaneTransport {
	return current;
}

/**
 * Tauri-`invoke`-compatible shim routed through the active transport. Keeps
 * existing `invoke<T>('cmd', args)` call sites working verbatim.
 */
export function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
	return current.invoke(cmd as never, args as never) as Promise<T>;
}

/**
 * Tauri-`listen`-compatible shim. Adapts the transport's payload-first callback
 * back to Tauri's `{ payload }` event shape so `listen(event, e => e.payload)`
 * call sites are unchanged.
 */
export function listen<T = unknown>(
	event: string,
	cb: (event: { payload: T }) => void
): Promise<Unsubscribe> {
	return current.subscribe(event as keyof ControlPlaneEvents, (payload) =>
		cb({ payload: payload as T })
	);
}
