export type {
	Capabilities,
	ControlPlaneCommands,
	ControlPlaneEvents,
	ControlPlaneTransport,
	Unsubscribe
} from './transport.ts';
export { createTauriTransport } from './tauri.ts';
export { createHttpTransport, type HttpTransportOptions } from './http.ts';
export { DEFAULT_TIMEOUT_MS, SLOW_TIMEOUT_MS, withTimeout } from './fetch-timeout.ts';
export { createMockTransport, type MockTransport } from './mock.ts';
export { parseTerminalControlFrame, type TerminalControlFrame } from './terminal-frames.ts';
export { buildPairingUri, isStrongToken, parsePairingUri, type PairingInfo } from './pairing.ts';

/**
 * Build the terminal-attach WebSocket URL. The server reads only the id from the
 * path (initial size is sent via the resize message on open). A browser
 * WebSocket can't set an Authorization header, so the bearer token — when the
 * server was started with one — rides along as a `?token=` query param.
 */
export function terminalWsUrl(serverUrl: string, id: string, token?: string): string {
	return wsUrl(serverUrl, `/remote/terminals/${id}/ws`, token);
}

/**
 * WebSocket URL of a Claude chat session (`/agent/claude/:id/ws`). `meta=changed`:
 * `update` frames carry `meta` only when it changed (the chat keeps the last one).
 */
export function agentWsUrl(serverUrl: string, sessionId: string, token?: string): string {
	const url = wsUrl(serverUrl, `/agent/claude/${encodeURIComponent(sessionId)}/ws`, token);
	return `${url}${url.includes('?') ? '&' : '?'}meta=changed`;
}

function wsUrl(serverUrl: string, path: string, token?: string): string {
	const base = serverUrl.replace(/^http/, 'ws').replace(/\/$/, '');
	const qs = token ? `?token=${encodeURIComponent(token)}` : '';
	return `${base}${path}${qs}`;
}
