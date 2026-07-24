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
	 * configured. Used to build the `?token=` query for WS attach URLs. Always
	 * null for the loopback server (no auth needed on 127.0.0.1).
	 */
	token: string | null;
}

export function startServer(port: number, token?: string): Promise<ServerStatus> {
	return invoke<ServerStatus>('start_server', { port, token: token ?? null });
}

export function stopServer(): Promise<ServerStatus> {
	return invoke<ServerStatus>('stop_server');
}

/** Status of the opt-in LAN server (server-mode settings UI). */
export function serverStatus(): Promise<ServerStatus> {
	return invoke<ServerStatus>('server_status');
}

/**
 * Status of the always-on loopback server that hosts desktop xterm PTYs. This
 * is the one `TerminalConnection` attaches to — NOT the opt-in LAN server (which
 * is off by default), so terminals work regardless of server-mode state.
 */
export function terminalServerStatus(): Promise<ServerStatus> {
	return invoke<ServerStatus>('terminal_server_status');
}

/**
 * Hook-bridge socket address (`127.0.0.1:<port>`), forwarded to the embedded
 * server so server-hosted xterm panes set `WORKBENCH_HOOK_SOCKET` and the
 * Claude/Codex hook bridge fires for them. Null if the bridge failed to bind.
 */
export function terminalHookSocket(): Promise<string | null> {
	return invoke<string | null>('terminal_hook_socket');
}
