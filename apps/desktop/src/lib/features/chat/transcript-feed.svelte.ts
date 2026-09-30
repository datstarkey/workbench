import { transcriptWsUrl } from '@workbench/transport';
import type { TranscriptItem, TranscriptMeta, TranscriptServerMsg } from '$types/workbench';
import { resolveServer } from '$features/terminal/terminal-connection';
import { mergeTranscript } from './chat-format';

export type FeedStatus = 'connecting' | 'live' | 'closed';

const RETRY_MS = 2000;

async function loopbackTranscriptUrl(sessionId: string): Promise<string> {
	const { baseUrl, token } = await resolveServer();
	return transcriptWsUrl(baseUrl, sessionId, token);
}

/**
 * Live chat items for one Claude session, streamed from the loopback server
 * (which tails the session JSONL). Reconnects on drop; `dispose()` to stop.
 */
export class TranscriptFeed {
	items = $state.raw<TranscriptItem[]>([]);
	meta = $state.raw<TranscriptMeta | null>(null);
	/** The snapshot left out older items; they're still in the terminal. */
	truncated = $state(false);
	status = $state<FeedStatus>('connecting');

	private readonly sessionId: string;
	private readonly socketUrl: (sessionId: string) => Promise<string>;
	private ws: WebSocket | null = null;
	private retryTimer: ReturnType<typeof setTimeout> | null = null;
	private disposed = false;

	constructor(
		sessionId: string,
		socketUrl: (sessionId: string) => Promise<string> = loopbackTranscriptUrl
	) {
		this.sessionId = sessionId;
		this.socketUrl = socketUrl;
		void this.open();
	}

	private async open(): Promise<void> {
		let url: string;
		try {
			url = await this.socketUrl(this.sessionId);
		} catch {
			this.scheduleRetry();
			return;
		}
		if (this.disposed) return;
		const ws = new WebSocket(url);
		this.ws = ws;
		ws.onopen = () => {
			this.status = 'live';
		};
		ws.onmessage = (event) => {
			try {
				this.receive(JSON.parse(String(event.data)) as TranscriptServerMsg);
			} catch (e) {
				console.warn('[TranscriptFeed] bad frame', e);
			}
		};
		ws.onclose = () => {
			if (this.ws === ws) this.ws = null;
			if (this.status !== 'closed') this.scheduleRetry();
		};
	}

	receive(msg: TranscriptServerMsg): void {
		if (msg.t === 'revoked') {
			this.status = 'closed';
			this.ws?.close();
			return;
		}
		this.items = mergeTranscript(this.items, msg);
		this.meta = msg.meta;
		if (msg.t === 'snapshot') this.truncated = msg.truncated;
	}

	private scheduleRetry(): void {
		if (this.disposed) return;
		this.status = 'connecting';
		this.retryTimer = setTimeout(() => void this.open(), RETRY_MS);
	}

	dispose(): void {
		this.disposed = true;
		this.status = 'closed';
		if (this.retryTimer) clearTimeout(this.retryTimer);
		this.ws?.close();
		this.ws = null;
	}
}
