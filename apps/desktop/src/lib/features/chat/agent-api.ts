import { agentWsUrl } from '@workbench/transport';
import type { StartAgentBody } from '$types/workbench';
import { resolveServer } from '$features/terminal/terminal-connection';
import { terminalHookSocket } from '$lib/server-mode';

/** The loopback server calls a chat session needs; injectable for tests. */
export interface AgentApi {
	start(body: StartAgentBody): Promise<void>;
	socketUrl(sessionId: string): Promise<string>;
}

async function call(method: string, path: string, body?: unknown): Promise<void> {
	const { baseUrl, token } = await resolveServer();
	const resp = await fetch(`${baseUrl}${path}`, {
		method,
		headers: {
			...(token ? { authorization: `Bearer ${token}` } : {}),
			...(body ? { 'content-type': 'application/json' } : {})
		},
		body: body ? JSON.stringify(body) : undefined
	});
	if (!resp.ok) {
		const message = await resp
			.json()
			.then((j: { error?: string }) => j.error)
			.catch(() => undefined);
		throw new Error(message || `${resp.status} ${resp.statusText}`);
	}
}

export const loopbackAgentApi: AgentApi = {
	async start(body) {
		// Hooks then report this session's activity to the sidebar, as for terminals.
		const hookSocket = body.hookSocket ?? (await terminalHookSocket().catch(() => null));
		await call('POST', '/agent/claude', { ...body, hookSocket: hookSocket ?? undefined });
	},
	async socketUrl(sessionId) {
		const { baseUrl, token } = await resolveServer();
		return agentWsUrl(baseUrl, sessionId, token);
	}
};

/** Stop a chat session's `claude` process, e.g. before the terminal takes it over. */
export function stopAgent(sessionId: string): Promise<void> {
	return call('DELETE', `/agent/claude/${encodeURIComponent(sessionId)}`);
}

/** Stop whatever chat session a closed pane owned. Best-effort. */
export function stopAgentForPane(paneId: string): Promise<void> {
	return call('DELETE', `/agent/claude?paneId=${encodeURIComponent(paneId)}`).catch(() => {});
}
