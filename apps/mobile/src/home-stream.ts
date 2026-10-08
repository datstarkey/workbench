import type { AgentSummary, ServerTerminalMeta as TerminalMeta } from '@workbench/types';

/** Retry delays after a failed stream; the poll covers the gap. */
const BASE_RETRY_MS = 2000;
const MAX_RETRY_MS = 60_000;
/** The server pings after 15s of quiet: this long without any event is a dead link. */
const STALE_MS = 45_000;

export interface HomeStreamHandlers {
	agents(list: AgentSummary[]): void;
	terminals(list: TerminalMeta[]): void;
	/** The stream started delivering (true) or failed and will retry (false). */
	status(live: boolean): void;
}

export type OpenEventSource = (url: string) => EventSource;

/**
 * The server's home lists over Server-Sent Events (`GET /events/home`). While
 * `live` is false (connecting, failed, or a host too old to have the route)
 * the caller keeps polling. Retries back off, so an older host costs one
 * request a minute at most.
 */
export class HomeStream {
	/** True once the current stream delivered its first event. */
	live = false;
	private source: EventSource | null = null;
	private url: string | null = null;
	private retryTimer: ReturnType<typeof setTimeout> | null = null;
	private staleTimer: ReturnType<typeof setTimeout> | null = null;
	private retryMs = BASE_RETRY_MS;

	constructor(
		private readonly handlers: HomeStreamHandlers,
		private readonly openSource: OpenEventSource = (url) => new EventSource(url)
	) {}

	/** Stream from this server, or stop with null. Following the same server again is a no-op. */
	follow(server: { url: string; token: string } | null): void {
		const url = server
			? `${server.url}/events/home?token=${encodeURIComponent(server.token)}`
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
		const current = () => this.source === source;
		const list =
			<T>(deliver: (list: T[]) => void) =>
			(ev: MessageEvent) => {
				if (!current()) return;
				let data: unknown;
				try {
					data = JSON.parse(ev.data);
				} catch {
					return;
				}
				this.heard();
				if (Array.isArray(data)) deliver(data as T[]);
			};
		source.addEventListener('agents', list(this.handlers.agents));
		source.addEventListener('terminals', list(this.handlers.terminals));
		source.addEventListener('ping', () => {
			if (current()) this.heard();
		});
		// EventSource retries some failures by itself and gives up on others (a
		// 404 from an older host); always take over with our own backoff.
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
