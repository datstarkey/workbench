import { agentClient, type AgentApi } from '@workbench/chat-ui';
import { resolveServer } from '$features/terminal/terminal-connection';
import { terminalHookSocket } from '$lib/server-mode';
import type { AgentSummary } from '$types/workbench';

const loopback = agentClient(resolveServer);

/** Chat sessions on the loopback server. */
export const loopbackAgentApi: AgentApi = {
	async start(body) {
		// Hooks then report this session's activity to the sidebar, as for terminals.
		const hookSocket = body.hookSocket ?? (await terminalHookSocket().catch(() => null));
		await loopback.start({ ...body, hookSocket: hookSocket ?? undefined });
	},
	socketUrl: loopback.socketUrl
};

/** A Claude account's plan limits, cached by the loopback server. */
export const planUsage = loopback.usage;

/** Stop a chat session's `claude` process, e.g. before the terminal takes it over. */
export const stopAgent = loopback.stop;

/** Live chat sessions on the loopback server, or null when it can't be reached. */
export function listAgents(): Promise<AgentSummary[] | null> {
	return loopback.list().catch(() => null);
}

/** Stop whatever chat session a closed pane owned. Best-effort. */
export function stopAgentForPane(paneId: string): Promise<void> {
	return loopback.stopPane(paneId).catch(() => {});
}
