import { agentWsUrl, DEFAULT_TIMEOUT_MS, SLOW_TIMEOUT_MS, withTimeout } from '@workbench/transport';
import type {
	AgentClientMsg,
	AgentSummary,
	ChatTarget,
	TaskTranscript,
	UsageLimit
} from '@workbench/types';

/** What an {@link AgentChat} needs from the server; injectable for tests. */
export interface AgentApi {
	/**
	 * The running session's current id (it follows a `/clear`). Rejects with
	 * `status: 404` while none runs: starting one is a workspace command.
	 */
	attach(sessionId: string): Promise<string>;
	socketUrl(sessionId: string): Promise<string>;
	/** The chat cwd's files for `@` mentions; absent leaves the menu out. */
	files?(where: Pick<ChatTarget, 'projectPath' | 'worktreePath'>): Promise<string[]>;
	/** A subagent's own conversation; null until the CLI writes it. */
	taskTranscript?(sessionId: string, taskId: string): Promise<TaskTranscript | null>;
}

/** A plan-usage check can wait on a `claude -p /usage` run. */
const USAGE_TIMEOUT_MS = 60_000;
/** Listing a big repo's files for the `@` menu. */
const FILES_TIMEOUT_MS = 20_000;

export interface AgentServer {
	baseUrl: string;
	token?: string | null;
}

/**
 * The chat-session routes of a Workbench server. `server` is read on every
 * call, so a new address or rotated token applies without rebuilding this.
 */
export function agentClient(server: () => AgentServer | Promise<AgentServer>) {
	async function call<T>(
		method: string,
		path: string,
		body?: unknown,
		timeoutMs = DEFAULT_TIMEOUT_MS
	): Promise<T | null> {
		const { baseUrl, token } = await server();
		return withTimeout(`${method} ${path.split('?')[0]}`, timeoutMs, async (signal) => {
			const resp = await fetch(`${baseUrl}${path}`, {
				method,
				headers: {
					...(token ? { authorization: `Bearer ${token}` } : {}),
					...(body ? { 'content-type': 'application/json' } : {})
				},
				body: body ? JSON.stringify(body) : undefined,
				signal
			});
			if (!resp.ok) {
				const err = await resp
					.json()
					.then((j: { error?: string }) => j)
					.catch(() => undefined);
				throw Object.assign(new Error(err?.error || `${resp.status} ${resp.statusText}`), {
					status: resp.status
				});
			}
			return resp.status === 204 ? null : ((await resp.json()) as T);
		});
	}
	const path = (id: string) => `/agent/claude/${encodeURIComponent(id)}`;

	async function list(): Promise<AgentSummary[]> {
		return (await call<AgentSummary[]>('GET', '/agent')) ?? [];
	}

	return {
		async attach(sessionId: string): Promise<string> {
			const running = (await list()).find(
				(s) => s.sessionId === sessionId || s.previousIds?.includes(sessionId)
			);
			if (!running) throw Object.assign(new Error('The session is not running.'), { status: 404 });
			return running.sessionId;
		},
		async socketUrl(sessionId: string): Promise<string> {
			const { baseUrl, token } = await server();
			return agentWsUrl(baseUrl, sessionId, token ?? undefined);
		},
		async files({
			projectPath,
			worktreePath
		}: Pick<ChatTarget, 'projectPath' | 'worktreePath'>): Promise<string[]> {
			const query = new URLSearchParams({
				projectPath,
				...(worktreePath ? { worktreePath } : {})
			}).toString();
			return (
				(await call<string[]>('GET', `/agent/files?${query}`, undefined, FILES_TIMEOUT_MS)) ?? []
			);
		},
		taskTranscript(sessionId: string, taskId: string): Promise<TaskTranscript | null> {
			return call<TaskTranscript>(
				'GET',
				`${path(sessionId)}/tasks/${encodeURIComponent(taskId)}/transcript`
			);
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
			const route = `/agent/usage${query ? `?${query}` : ''}`;
			return (await call<UsageLimit[]>('GET', route, undefined, USAGE_TIMEOUT_MS)) ?? [];
		},
		/** One message without a socket, e.g. answering an approval from a list. */
		async send(sessionId: string, msg: AgentClientMsg): Promise<void> {
			await call('POST', `${path(sessionId)}/message`, msg, SLOW_TIMEOUT_MS);
		}
	} satisfies AgentApi & Record<string, unknown>;
}

export type AgentClient = ReturnType<typeof agentClient>;
