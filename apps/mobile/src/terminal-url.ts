/**
 * Build the terminal-attach WebSocket URL. The server reads only the id from the
 * path (initial size is sent via the resize message on open). A browser
 * WebSocket can't set an Authorization header, so the bearer token — when the
 * server was started with one — rides along as a `?token=` query param.
 */
export function terminalWsUrl(serverUrl: string, id: string, token?: string): string {
	const base = serverUrl.replace(/^http/, 'ws').replace(/\/$/, '');
	const qs = token ? `?token=${encodeURIComponent(token)}` : '';
	return `${base}/remote/terminals/${id}/ws${qs}`;
}
