import type { WorkspaceCommand, WorkspaceCommandResult, WorkspaceSnapshot } from '@workbench/types';
import { SLOW_TIMEOUT_MS, withTimeout } from './fetch-timeout.ts';
import type { Unsubscribe } from './transport.ts';

/** Where the workspace service is: a remote server, or the desktop's loopback one. */
export interface ServerAddress {
	baseUrl: string;
	token?: string;
}

export type OpenEventSource = (url: string) => EventSource;

export interface WorkspaceStreamHandlers {
	/** `fresh`: the first frame of a new connection, which may restart `rev` (a restarted server). */
	snapshot(snapshot: WorkspaceSnapshot, fresh: boolean): void;
	/** The stream started delivering (true) or failed and will retry (false). */
	status?(live: boolean): void;
}

const BASE_RETRY_MS = 1000;
const MAX_RETRY_MS = 30_000;
/** The server pings after 15s of quiet: this long without any event is a dead link. */
const STALE_MS = 45_000;

/**
 * The workspace model over Server-Sent Events (`GET /events/workspace`): a full
 * snapshot on connect and on every change. Frames whose `rev` isn't newer than
 * the last one on the same connection are dropped. A failed or silent stream
 * reconnects with backoff.
 */
export class WorkspaceStream {
	live = false;
	private source: EventSource | null = null;
	private retryTimer: ReturnType<typeof setTimeout> | null = null;
	private staleTimer: ReturnType<typeof setTimeout> | null = null;
	private retryMs = BASE_RETRY_MS;
	private rev: number | null = null;
	private stopped = false;

	constructor(
		private readonly server: () => Promise<ServerAddress>,
		private readonly handlers: WorkspaceStreamHandlers,
		private readonly openSource: OpenEventSource = (url) => new EventSource(url)
	) {
		void this.connect();
	}

	private async connect(): Promise<void> {
		let url: string;
		try {
			const { baseUrl, token } = await this.server();
			const base = baseUrl.replace(/\/$/, '');
			url = `${base}/events/workspace${token ? `?token=${encodeURIComponent(token)}` : ''}`;
		} catch {
			return this.fail();
		}
		if (this.stopped || typeof EventSource === 'undefined') return;
		const source = this.openSource(url);
		this.source = source;
		this.rev = null;
		const current = () => this.source === source;
		source.addEventListener('snapshot', (ev) => {
			if (!current()) return;
			let snapshot: WorkspaceSnapshot;
			try {
				snapshot = JSON.parse((ev as MessageEvent).data);
			} catch {
				return;
			}
			this.heard();
			if (!Array.isArray(snapshot?.workspaces) || typeof snapshot.rev !== 'number') return;
			const fresh = this.rev === null;
			if (!fresh && snapshot.rev <= this.rev!) return;
			this.rev = snapshot.rev;
			this.handlers.snapshot(snapshot, fresh);
		});
		source.addEventListener('ping', () => {
			if (current()) this.heard();
		});
		// EventSource retries some failures itself and gives up on others (a 401):
		// always take over with our own backoff.
		source.addEventListener('error', () => {
			if (current()) this.fail();
		});
		this.armStale();
	}

	private heard(): void {
		this.armStale();
		if (this.live) return;
		this.live = true;
		this.retryMs = BASE_RETRY_MS;
		this.handlers.status?.(true);
	}

	private armStale(): void {
		if (this.staleTimer) clearTimeout(this.staleTimer);
		this.staleTimer = setTimeout(() => this.fail(), STALE_MS);
	}

	private fail(): void {
		if (this.stopped) return;
		this.close();
		this.handlers.status?.(false);
		this.retryTimer = setTimeout(() => {
			this.retryTimer = null;
			void this.connect();
		}, this.retryMs);
		this.retryMs = Math.min(this.retryMs * 2, MAX_RETRY_MS);
	}

	private close(): void {
		this.source?.close();
		this.source = null;
		this.live = false;
		if (this.retryTimer) clearTimeout(this.retryTimer);
		if (this.staleTimer) clearTimeout(this.staleTimer);
		this.retryTimer = this.staleTimer = null;
	}

	stop(): void {
		this.stopped = true;
		this.close();
	}
}

/** 32 hex chars; `crypto.randomUUID` needs a secure context, which a phone's WebView isn't. */
function requestId(): string {
	return Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) =>
		b.toString(16).padStart(2, '0')
	).join('');
}

/**
 * `POST /workspace/commands`; a refused command throws the server's reason.
 * A `newSession` carries a `requestId` and is sent once more when the first
 * try got no answer (a slow link): the server applies it once either way.
 */
export async function sendWorkspaceCommand(
	server: ServerAddress,
	cmd: WorkspaceCommand
): Promise<WorkspaceCommandResult> {
	if (cmd.type !== 'newSession') return postCommand(server, cmd);
	const once = { ...cmd, requestId: cmd.requestId ?? requestId() };
	try {
		return await postCommand(server, once);
	} catch (e) {
		if ((e as { status?: number }).status !== undefined) throw e;
		return postCommand(server, once);
	}
}

async function postCommand(
	server: ServerAddress,
	cmd: WorkspaceCommand
): Promise<WorkspaceCommandResult> {
	const base = server.baseUrl.replace(/\/$/, '');
	return withTimeout(
		'workbench-server: POST /workspace/commands',
		SLOW_TIMEOUT_MS,
		async (signal) => {
			const res = await fetch(`${base}/workspace/commands`, {
				method: 'POST',
				headers: {
					'content-type': 'application/json',
					...(server.token ? { authorization: `Bearer ${server.token}` } : {})
				},
				body: JSON.stringify(cmd),
				signal
			});
			let body: (WorkspaceCommandResult & { error?: string }) | null = null;
			try {
				body = await res.json();
			} catch {
				/* keep the status */
			}
			if (!res.ok || body?.error) {
				const message = body?.error ?? `${res.status} ${res.statusText}`;
				throw Object.assign(new Error(message), { status: res.status });
			}
			return body as WorkspaceCommandResult;
		}
	);
}

/** The two workspace methods of a transport, against a server found by `server()`. */
export function workspaceMethods(
	server: () => Promise<ServerAddress>,
	openSource?: OpenEventSource
) {
	return {
		async workspaceCommand(cmd: WorkspaceCommand): Promise<WorkspaceCommandResult> {
			return sendWorkspaceCommand(await server(), cmd);
		},
		subscribeWorkspace(handlers: WorkspaceStreamHandlers): Unsubscribe {
			const stream = new WorkspaceStream(server, handlers, openSource);
			return () => stream.stop();
		}
	};
}
