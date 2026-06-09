/**
 * Desktop "server mode" controls. These talk to the LOCAL Tauri backend (which
 * embeds the Workbench control-plane server) — they are not control-plane
 * commands and must not be routed through the transport abstraction.
 */
import { invoke } from '@tauri-apps/api/core';

export interface ServerStatus {
	running: boolean;
	address: string | null;
	/**
	 * Bearer token used by the embedded server, or null when no auth is
	 * configured. The frontend uses this to build the `?token=` query
	 * parameter for WebSocket terminal attach URLs.
	 *
	 * Will be populated once `server_control.rs` is extended in the
	 * desktop-boot batch to include the token in `ServerStatus`.
	 */
	token: string | null;
}

export function startServer(port: number, token?: string): Promise<ServerStatus> {
	return invoke<ServerStatus>('start_server', { port, token: token ?? null });
}

export function stopServer(): Promise<ServerStatus> {
	return invoke<ServerStatus>('stop_server');
}

export function serverStatus(): Promise<ServerStatus> {
	return invoke<ServerStatus>('server_status');
}
