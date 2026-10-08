import { agentWsUrl, DEFAULT_TIMEOUT_MS, withTimeout } from '@workbench/transport';
import type {
	AgentClientMsg,
	AgentSummary,
	StartAgentBody,
	TaskTranscript,
	UsageLimit
} from '@workbench/types';

/** Claude Code asks to trust the chat's folder before it starts; `path` is that folder. */
export class NeedsTrustError extends Error {
	constructor(readonly path: string) {
		super(`Claude Code needs you to trust ${path} first`);
	}
}

/** What an {@link AgentChat} needs from the server; injectable for tests. */
export interface AgentApi {
	/**
	 * Resolves to the session's id: a new Codex thread only gets one here.
	 * Rejects with {@link NeedsTrustError} while Claude Code waits on its folder trust dialog.
	 */
	start(body: StartAgentBody): Promise<string>;
	socketUrl(sessionId: string): Promise<string>;
	/** The server terminal a started Claude chat runs in (its interactive `claude`). */
	terminalId?(sessionId: string): string | undefined;
	/** The chat cwd's files for `@` mentions; absent leaves the menu out. */
	files?(where: Pick<StartAgentBody, 'projectPath' | 'worktreePath'>): Promise<string[]>;
	/** A subagent's own conversation; null until the CLI writes it. */
	taskTranscript?(sessionId: string, taskId: string): Promise<TaskTranscript | null>;
}

/**
 * A start waits for the session to come up: Claude's plugin to attach (up to
 * 30s, after a trust dialog or behind another start of the same id), or a
 * Codex thread's id (up to 30s).
 */
const START_TIMEOUT_MS = 90_000;
/** Stopping waits for the process to exit; a message may carry a 2 MB image. */
const SLOW_TIMEOUT_MS = 30_000;
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
					.then((j: { error?: string; ended?: boolean }) => j)
					.catch(() => undefined);
				// `ended`: an attach-only start on a chat someone ended.
				throw Object.assign(new Error(err?.error || `${resp.status} ${resp.statusText}`), {
					status: resp.status,
					ended: err?.ended === true
				});
			}
			return resp.status === 204 ? null : ((await resp.json()) as T);
		});
	}
	const path = (id: string) => `/agent/claude/${encodeURIComponent(id)}`;
	const terminals = new Map<string, string>();

	return {
		async start(body: StartAgentBody): Promise<string> {
			const res = await call<{
				sessionId: string;
				terminalId?: string | null;
				needsTrust?: string;
			}>('POST', `/agent/${body.agent ?? 'claude'}`, body, START_TIMEOUT_MS);
			if (res?.needsTrust) throw new NeedsTrustError(res.needsTrust);
			if (!res?.sessionId) throw new Error('The server did not return a session id');
			if (res.terminalId) terminals.set(res.sessionId, res.terminalId);
			return res.sessionId;
		},
		terminalId: (sessionId: string) => terminals.get(sessionId),
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
		 * Stop a session's process, e.g. before a terminal takes it over. `end`:
		 * the person ended the chat, so other devices close it too.
		 */
		async stop(sessionId: string, opts?: { end?: boolean }): Promise<void> {
			await call(
				'DELETE',
				`${path(sessionId)}${opts?.end ? '?end=true' : ''}`,
				undefined,
				SLOW_TIMEOUT_MS
			);
		},
		/** Stop whatever chat session a pane owned; `end` as for `stop`. */
		async stopPane(paneId: string, opts?: { end?: boolean }): Promise<void> {
			await call(
				'DELETE',
				`/agent/claude?paneId=${encodeURIComponent(paneId)}${opts?.end ? '&end=true' : ''}`,
				undefined,
				SLOW_TIMEOUT_MS
			);
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
