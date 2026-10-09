import type { AgentSummary } from '@workbench/types';

/*
 * The server-owned workspace model as the phone needs it: `GET /events/workspace`
 * and `POST /workspace/commands` (see docs/WORKSPACE_MODEL.md). Self-contained so
 * the shared `@workbench/transport` version can replace it with one import.
 */

export type PaneKind = 'shell' | 'claude' | 'codex';
export type CodexMode = 'tui' | 'appServer';
export type PaneStatus = 'starting' | 'needsTrust' | 'running' | 'exited';

/** A pane with its runtime state merged in, as a snapshot frame carries it. */
export interface WorkspacePane {
	id: string;
	kind: PaneKind;
	sessionId?: string;
	previousIds?: string[];
	accountId?: string;
	codexMode?: CodexMode;
	terminalId: string | null;
	status: PaneStatus;
	title: string | null;
	busy: boolean;
	waiting: AgentSummary['waiting'];
	error: string | null;
}

export interface WorkspaceTab {
	id: string;
	label: string;
	kind: PaneKind;
	panes: WorkspacePane[];
}

export interface Workspace {
	id: string;
	projectPath: string;
	projectName: string;
	worktreePath?: string;
	branch?: string;
	tabs: WorkspaceTab[];
}

export interface WorkspaceSnapshot {
	rev: number;
	workspaces: Workspace[];
}

/** Where a new session goes: a workspace, or a folder (its workspace is opened if needed). */
export type SessionTarget =
	| { workspaceId: string }
	| { projectPath: string; worktreePath?: string; projectName?: string; branch?: string };

/** The commands the phone sends. */
export type WorkspaceCommand =
	| (SessionTarget & {
			type: 'newSession';
			kind: PaneKind;
			resume?: string;
			accountId?: string;
			codexMode?: CodexMode;
	  })
	| { type: 'closePane'; paneId: string }
	| { type: 'closeTab'; tabId: string }
	| { type: 'restart'; tabId: string }
	| { type: 'trustFolder'; paneId: string };

/** Answered once a snapshot at `rev` shows the command. */
export interface CommandResult {
	rev: number;
	workspaceId?: string | null;
	tabId?: string | null;
	paneId?: string | null;
}

export interface WorkspaceServer {
	url: string;
	token: string;
}

/** Send one command; rejects with the server's reason when the model refuses it. */
export async function workspaceCommand(
	server: WorkspaceServer,
	cmd: WorkspaceCommand,
	fetchImpl: typeof fetch = fetch
): Promise<CommandResult> {
	const res = await fetchImpl(`${server.url}/workspace/commands`, {
		method: 'POST',
		headers: { 'content-type': 'application/json', authorization: `Bearer ${server.token}` },
		body: JSON.stringify(cmd),
		signal: AbortSignal.timeout(30_000)
	});
	const body = (await res.json().catch(() => null)) as (CommandResult & { error?: string }) | null;
	if (!res.ok || !body) throw new Error(body?.error || `the server returned ${res.status}`);
	return body;
}

const BASE_RETRY_MS = 1000;
const MAX_RETRY_MS = 30_000;
/** The server pings after 15s of quiet: this long without any event is a dead link. */
const STALE_MS = 45_000;

export interface WorkspaceStreamHandlers {
	snapshot(snapshot: WorkspaceSnapshot): void;
	/** The stream started delivering (true) or failed and will retry (false). */
	status(live: boolean): void;
}

export type OpenEventSource = (url: string) => EventSource;

/**
 * Follows `/events/workspace`, reconnecting with backoff. Within one
 * connection a frame whose `rev` isn't newer than the last is dropped; a new
 * connection's first frame is always taken (a restarted server counts again).
 */
export class WorkspaceStream {
	live = false;
	private source: EventSource | null = null;
	private url: string | null = null;
	private retryTimer: ReturnType<typeof setTimeout> | null = null;
	private staleTimer: ReturnType<typeof setTimeout> | null = null;
	private retryMs = BASE_RETRY_MS;
	private lastRev: number | null = null;

	constructor(
		private readonly handlers: WorkspaceStreamHandlers,
		private readonly openSource: OpenEventSource = (url) => new EventSource(url)
	) {}

	/** Stream from this server, or stop with null. Following the same server again is a no-op. */
	follow(server: WorkspaceServer | null): void {
		const url = server
			? `${server.url}/events/workspace?token=${encodeURIComponent(server.token)}`
			: null;
		if (url === this.url) return;
		this.stop();
		this.url = url;
		this.retryMs = BASE_RETRY_MS;
		if (url) this.connect();
	}

	private connect(): void {
		if (!this.url || typeof EventSource === 'undefined') return;
		const source = this.openSource(this.url);
		this.source = source;
		this.lastRev = null;
		const current = () => this.source === source;
		source.addEventListener('snapshot', (ev: MessageEvent) => {
			if (!current()) return;
			let data: WorkspaceSnapshot;
			try {
				data = JSON.parse(ev.data);
			} catch {
				return;
			}
			this.heard();
			if (typeof data?.rev !== 'number' || !Array.isArray(data.workspaces)) return;
			if (this.lastRev !== null && data.rev <= this.lastRev) return;
			this.lastRev = data.rev;
			this.handlers.snapshot(data);
		});
		source.addEventListener('ping', () => {
			if (current()) this.heard();
		});
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
		this.handlers.status(true);
	}

	private armStale(): void {
		if (this.staleTimer) clearTimeout(this.staleTimer);
		this.staleTimer = setTimeout(() => this.fail(), STALE_MS);
	}

	private fail(): void {
		const url = this.url;
		this.stop();
		this.url = url;
		this.handlers.status(false);
		this.retryTimer = setTimeout(() => {
			this.retryTimer = null;
			this.connect();
		}, this.retryMs);
		this.retryMs = Math.min(this.retryMs * 2, MAX_RETRY_MS);
	}

	private stop(): void {
		this.source?.close();
		this.source = null;
		this.url = null;
		this.live = false;
		if (this.retryTimer) clearTimeout(this.retryTimer);
		if (this.staleTimer) clearTimeout(this.staleTimer);
		this.retryTimer = this.staleTimer = null;
	}
}
