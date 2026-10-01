import type {
	AgentClientMsg,
	AgentServerMsg,
	ApprovalDecision,
	PermissionMode,
	StartAgentBody,
	TranscriptItem,
	TranscriptMeta
} from '$types/workbench';
import { uid } from '$lib/utils/uid';
import { type AgentApi, loopbackAgentApi } from './agent-api';
import { applyChanges } from './chat-format';

/**
 * - `starting`: launching or resuming the `claude` process.
 * - `live`: attached; prompts go straight to Claude.
 * - `reconnecting`: the socket dropped; Claude keeps running server-side.
 * - `exited`: the process ended (crash, `/exit`, server stopped).
 * - `failed`: it could not be started at all.
 */
export type ChatStatus = 'starting' | 'live' | 'reconnecting' | 'exited' | 'failed';

/** A prompt shown at once, until Claude echoes it back as a real item. */
export interface PendingPrompt {
	id: string;
	text: string;
	/** User items already in the chat when it was sent; only later ones can echo it. */
	after: number;
}

const RECONNECT_MS = 1500;

/** One chat pane's connection to its `claude -p` session on the loopback server. */
export class AgentChat {
	items = $state.raw<TranscriptItem[]>([]);
	meta = $state.raw<TranscriptMeta | null>(null);
	/** First item index held; above zero, older history was left out. */
	start = $state(0);
	status = $state<ChatStatus>('starting');
	/** Why the session failed or ended. */
	error = $state<string | null>(null);
	/** A rejected action (bad mode, write failed); cleared on the next success. */
	notice = $state<string | null>(null);
	pending = $state.raw<PendingPrompt[]>([]);
	/** When the current turn started (client clock), for the elapsed timer. */
	busySince = $state<number | null>(null);
	/** The session continued under a new id (`/clear`); the pane follows it. */
	sessionId = $state('');

	private readonly body: StartAgentBody;
	private readonly api: AgentApi;
	private ws: WebSocket | null = null;
	private retryTimer: ReturnType<typeof setTimeout> | null = null;
	private disposed = false;

	constructor(body: StartAgentBody, api: AgentApi = loopbackAgentApi) {
		this.body = body;
		this.api = api;
		this.sessionId = body.sessionId;
		void this.open();
	}

	/** Start (or resume) the process, then attach. Also the "Restart" action. */
	open(): Promise<void> {
		this.status = 'starting';
		this.error = null;
		return this.connect();
	}

	/**
	 * Starting is idempotent server-side (it returns the running session), so
	 * every (re)connect starts first: after an app restart the process is gone
	 * and this brings it back with the conversation resumed.
	 */
	private async connect(): Promise<void> {
		let url: string;
		try {
			await this.api.start({ ...this.body, sessionId: this.sessionId });
			url = await this.api.socketUrl(this.sessionId);
		} catch (e) {
			if (this.disposed) return;
			this.status = 'failed';
			this.error = e instanceof Error ? e.message : String(e);
			return;
		}
		if (this.disposed) return;
		const ws = new WebSocket(url);
		this.ws = ws;
		ws.onmessage = (event) => {
			try {
				this.receive(JSON.parse(String(event.data)) as AgentServerMsg);
			} catch (e) {
				console.warn('[AgentChat] bad frame', e);
			}
		};
		ws.onclose = () => {
			if (this.ws === ws) this.ws = null;
			if (this.status === 'live' || this.status === 'starting') this.scheduleReconnect();
		};
	}

	private scheduleReconnect(): void {
		if (this.disposed) return;
		this.status = 'reconnecting';
		this.retryTimer = setTimeout(() => void this.connect(), RECONNECT_MS);
	}

	receive(msg: AgentServerMsg): void {
		switch (msg.t) {
			case 'snapshot':
				this.sessionId = msg.sessionId;
				this.start = msg.start;
				this.items = msg.items;
				this.setMeta(msg.meta);
				this.status = msg.exited ? 'exited' : 'live';
				this.settlePending();
				break;
			case 'update':
				this.items = applyChanges(this.items, this.start, msg.changes);
				this.setMeta(msg.meta);
				this.notice = null;
				this.settlePending();
				break;
			case 'exit':
				this.status = 'exited';
				this.error = msg.message;
				this.pending = [];
				this.busySince = null;
				break;
			case 'error':
				this.notice = msg.message;
				break;
			case 'revoked':
				this.status = 'exited';
				this.error = 'The connection to Workbench was closed.';
				this.ws?.close();
				break;
		}
	}

	private setMeta(meta: TranscriptMeta): void {
		if (meta.busy && !this.meta?.busy) this.busySince = Date.now();
		if (!meta.busy) this.busySince = null;
		this.meta = meta;
	}

	/** Drop optimistic prompts Claude has echoed back. */
	private settlePending(): void {
		if (this.pending.length === 0) return;
		const users = this.userTexts();
		this.pending = this.pending.filter((p) => !users.slice(p.after).includes(p.text));
	}

	private userTexts(): string[] {
		return this.items.flatMap((i) => (i.kind === 'user' ? [i.text] : []));
	}

	private send(msg: AgentClientMsg): boolean {
		if (this.ws?.readyState !== WebSocket.OPEN) {
			this.notice = 'Not connected to Claude yet. Try again in a moment.';
			return false;
		}
		this.ws.send(JSON.stringify(msg));
		return true;
	}

	prompt(text: string): boolean {
		const trimmed = text.trim();
		if (!trimmed || !this.send({ t: 'prompt', text: trimmed })) return false;
		this.pending = [...this.pending, { id: uid(), text: trimmed, after: this.userTexts().length }];
		this.busySince ??= Date.now();
		return true;
	}

	approve(requestId: string, decision: ApprovalDecision, answers?: Record<string, string>): void {
		this.send({ t: 'approve', requestId, decision, ...(answers ? { answers } : {}) });
	}

	interrupt(): void {
		this.send({ t: 'interrupt' });
	}

	setMode(mode: PermissionMode): void {
		this.send({ t: 'mode', mode });
	}

	dispose(): void {
		this.disposed = true;
		if (this.retryTimer) clearTimeout(this.retryTimer);
		this.ws?.close();
		this.ws = null;
	}
}
