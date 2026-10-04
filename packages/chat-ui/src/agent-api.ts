import { agentWsUrl } from '@workbench/transport';
import type { AgentClientMsg, AgentSummary, StartAgentBody, UsageLimit } from '@workbench/types';

/** What an {@link AgentChat} needs from the server; injectable for tests. */
export interface AgentApi {
	/** Resolves to the session's id: a new Codex thread only gets one here. */
	start(body: StartAgentBody): Promise<string>;
	socketUrl(sessionId: string): Promise<string>;
	/** The chat cwd's files for `@` mentions; absent leaves the menu out. */
	files?(where: Pick<StartAgentBody, 'projectPath' | 'worktreePath'>): Promise<string[]>;
}

export interface AgentServer {
	baseUrl: string;
	token?: string | null;
}

/**
 * The chat-session routes of a Workbench server. `server` is read on every
 * call, so a new address or rotated token applies without rebuilding this.
 */
export function agentClient(server: () => AgentServer | Promise<AgentServer>) {
	async function call<T>(method: string, path: string, body?: unknown): Promise<T | null> {
		const { baseUrl, token } = await server();
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
			throw Object.assign(new Error(message || `${resp.status} ${resp.statusText}`), {
				status: resp.status
			});
		}
		return resp.status === 204 ? null : ((await resp.json()) as T);
	}
	const path = (id: string) => `/agent/claude/${encodeURIComponent(id)}`;

	return {
		async start(body: StartAgentBody): Promise<string> {
			const res = await call<{ sessionId: string }>(
				'POST',
				`/agent/${body.agent ?? 'claude'}`,
				body
			);
			if (!res?.sessionId) throw new Error('The server did not return a session id');
			return res.sessionId;
		},
		async socketUrl(sessionId: string): Promise<string> {
			const { baseUrl, token } = await server();
			return agentWsUrl(baseUrl, sessionId, token ?? undefined);
		},
		async files({
			projectPath,
			worktreePath
		}: Pick<StartAgentBody, 'projectPath' | 'worktreePath'>): Promise<string[]> {
			const query = new URLSearchParams({
				projectPath,
				...(worktreePath ? { worktreePath } : {})
			}).toString();
			return (await call<string[]>('GET', `/agent/files?${query}`)) ?? [];
		},
		/** Stop a session's process, e.g. before a terminal takes it over. */
		async stop(sessionId: string): Promise<void> {
			await call('DELETE', path(sessionId));
		},
		/** Stop whatever chat session a closed pane owned. */
		async stopPane(paneId: string): Promise<void> {
			await call('DELETE', `/agent/claude?paneId=${encodeURIComponent(paneId)}`);
		},
		/** Every live session, Claude and Codex. Servers older than Codex chat list Claude only. */
		async list(): Promise<AgentSummary[]> {
			try {
				return (await call<AgentSummary[]>('GET', '/agent')) ?? [];
			} catch (e) {
				if ((e as { status?: number }).status !== 404) throw e;
				const claude = (await call<AgentSummary[]>('GET', '/agent/claude')) ?? [];
				return claude.map((s) => ({ ...s, agent: 'claude' as const }));
			}
		},
		/**
		 * The account's plan limits; empty without a plan. Cached a minute
		 * server-side, or only a few seconds when `fresh` (a turn just ended).
		 */
		async usage(claudeAccountId?: string, fresh = false): Promise<UsageLimit[]> {
			const query = new URLSearchParams({
				...(claudeAccountId ? { claudeAccountId } : {}),
				...(fresh ? { fresh: 'true' } : {})
			}).toString();
			return (await call<UsageLimit[]>('GET', `/agent/usage${query ? `?${query}` : ''}`)) ?? [];
		},
		/** One message without a socket, e.g. answering an approval from a list. */
		async send(sessionId: string, msg: AgentClientMsg): Promise<void> {
			await call('POST', `${path(sessionId)}/message`, msg);
		}
	} satisfies AgentApi & Record<string, unknown>;
}

export type AgentClient = ReturnType<typeof agentClient>;
