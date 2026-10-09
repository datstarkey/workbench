import { agentClient, type AgentApi } from '@workbench/chat-ui';
import { resolveServer } from '$features/terminal/terminal-connection';
import type { AgentSummary } from '$types/workbench';

const loopback = agentClient(resolveServer);

/** Chat sessions on the loopback server, which reports their hooks itself. */
export const loopbackAgentApi: AgentApi = loopback;

/** A Claude account's plan limits, cached by the loopback server. */
export const planUsage = loopback.usage;

/** Stop a chat session's process, e.g. before the terminal takes it over. */
export const stopAgent = loopback.stop;

/** Live chat sessions on the loopback server, or null when it can't be reached. */
export function listAgents(): Promise<AgentSummary[] | null> {
	return loopback.list().catch(() => null);
}

/** Stop whatever chat session a pane owned; `end` when it closed. Best-effort. */
export function stopAgentForPane(paneId: string, opts?: { end?: boolean }): Promise<void> {
	return loopback.stopPane(paneId, opts).catch(() => {});
}
