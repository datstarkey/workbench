import { agentClient, type AgentApi } from '@workbench/chat-ui';
import { resolveServer } from '$features/terminal/terminal-connection';

const loopback = agentClient(resolveServer);

/** Chat sessions on the loopback server. */
export const loopbackAgentApi: AgentApi = loopback;

/** A Claude account's plan limits, cached by the loopback server. */
export const planUsage = loopback.usage;
