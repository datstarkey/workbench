import type {
	AgentClientMsg,
	AgentServerMsg,
	ApprovalDecision,
	ChatImage,
	EffortLevel,
	PermissionMode,
	SlashCommand,
	StartAgentBody,
	TranscriptItem,
	TranscriptMeta
} from '@workbench/types';
import type { AgentApi } from './agent-api';
import { applyChanges } from './chat-format';
import { previewUrl } from './image-intake';

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
	/** Data URLs of attached images, shown in the bubble. */
	previews: string[];
	/** User items already in the chat when it was sent; only later ones can echo it. */
	after: number;
}

const RECONNECT_MS = 1500;

export interface TaskOutput {
	text: string;
	/** Total size of the output so far. */
	bytes: number;
}

/** One chat view's connection to its `claude -p` session on a Workbench server. */
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
	/** Slash commands for the composer's `/` menu. */
	commands = $state.raw<SlashCommand[]>([]);
	/** Previews of images sent from here, by the user item that echoed them. */
	imagePreviews = $state.raw<Record<string, string[]>>({});
	/** When each task or running tool was first seen (client clock), for timers. */
	seenAt = $state.raw<Record<string, number>>({});

	private body: StartAgentBody;
	private readonly api: AgentApi;
	private ws: WebSocket | null = null;
	private retryTimer: ReturnType<typeof setTimeout> | null = null;
	private disposed = false;
	/** Bumped by every connect; an older one still awaiting the server gives up. */
	private generation = 0;
	/** Called when Claude starts or stops waiting on the person (approval, question). */
	onNeedsYou: ((waiting: boolean) => void) | null = null;
	/** An `attachOnly` chat that ended was restarted here: this device now owns it. */
	onTakeOver: (() => void) | null = null;
	private waitingOnYou = false;
	/** Callbacks waiting on `output` / `taskOutput` replies; not UI state, so not reactive. */
	private outputWaiters: Record<string, (text: string | null) => void> = {};
	private taskWaiters: Record<string, (out: TaskOutput | null) => void> = {};

	constructor(body: StartAgentBody, api: AgentApi) {
		this.body = body;
		this.api = api;
		this.sessionId = body.sessionId;
		void this.open();
	}

	/**
	 * Start (or resume) the process, then attach. Also the "Restart" action: an
	 * `attachOnly` chat re-attaches while it runs, and once ended it starts here.
	 */
	open(): Promise<void> {
		if (this.body.attachOnly && (this.status === 'exited' || this.status === 'failed')) {
			this.body = { ...this.body, attachOnly: false };
			this.onTakeOver?.();
		}
		this.ws?.close(); // a Restart must not leave the old socket behind
		this.ws = null;
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
		const generation = ++this.generation;
		const stale = () => this.disposed || generation !== this.generation;
		let url: string;
		try {
			await this.api.start({ ...this.body, sessionId: this.sessionId });
			url = await this.api.socketUrl(this.sessionId);
		} catch (e) {
			if (stale()) return;
			// Waking phones lose the network for a moment; keep retrying rather than give up.
			if (this.status === 'reconnecting') return this.scheduleReconnect();
			this.status = 'failed';
			this.error = e instanceof Error ? e.message : String(e);
			return;
		}
		if (stale()) return;
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
			if (this.ws !== ws) return; // replaced by a Restart
			this.ws = null;
			if (this.status !== 'exited' && this.status !== 'failed') this.scheduleReconnect();
		};
	}

	private scheduleReconnect(): void {
		if (this.disposed) return;
		this.status = 'reconnecting';
		if (this.retryTimer) clearTimeout(this.retryTimer);
		this.retryTimer = setTimeout(() => void this.connect(), RECONNECT_MS);
	}

	receive(msg: AgentServerMsg): void {
		switch (msg.t) {
			case 'snapshot':
				// `/clear` re-keys to a new, empty transcript. The CLI never echoes
				// the `/clear` itself; prompts queued after it echo in the new one.
				if (msg.sessionId !== this.sessionId) {
					const clear = this.pending.findIndex((p) => /^\/clear(\s|$)/.test(p.text));
					this.pending = this.pending.slice(clear + 1).map((p) => ({ ...p, after: 0 }));
				}
				this.sessionId = msg.sessionId;
				this.start = msg.start;
				this.items = msg.items;
				this.commands = msg.commands;
				this.setMeta(msg.meta);
				this.status = msg.exited ? 'exited' : 'live';
				this.settlePending();
				this.reportWaiting();
				break;
			case 'update':
				this.items = applyChanges(this.items, this.start, msg.changes);
				this.markSeen(
					msg.changes.flatMap(([, i]) =>
						i.kind === 'tool' && i.status === 'running' ? [i.id] : []
					)
				);
				this.setMeta(msg.meta);
				this.notice = null;
				this.settlePending();
				this.reportWaiting();
				break;
			case 'exit':
				this.status = 'exited';
				this.error = msg.message;
				this.pending = [];
				this.busySince = null;
				// Nothing can be answered now: stop flagging the pane as waiting.
				if (this.waitingOnYou) {
					this.waitingOnYou = false;
					this.onNeedsYou?.(false);
				}
				this.ws?.close();
				break;
			case 'error':
				this.notice = msg.message;
				break;
			case 'commands':
				this.commands = msg.commands;
				break;
			case 'output':
				this.outputWaiters[msg.toolId]?.(msg.text);
				delete this.outputWaiters[msg.toolId];
				break;
			case 'taskOutput':
				this.taskWaiters[msg.taskId]?.(
					msg.text === null ? null : { text: msg.text, bytes: msg.bytes ?? msg.text.length }
				);
				delete this.taskWaiters[msg.taskId];
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
		this.markSeen(meta.tasks.map((t) => t.id));
		this.meta = meta;
	}

	private markSeen(ids: string[]): void {
		const unseen = ids.filter((id) => !(id in this.seenAt));
		if (unseen.length === 0) return;
		const now = Date.now();
		this.seenAt = { ...this.seenAt, ...Object.fromEntries(unseen.map((id) => [id, now])) };
	}

	private reportWaiting(): void {
		const waiting = this.items.some((i) => i.kind === 'approval' && !i.decision && !i.expired);
		if (waiting === this.waitingOnYou) return;
		this.waitingOnYou = waiting;
		this.onNeedsYou?.(waiting);
	}

	/** Drop optimistic prompts Claude has echoed back. */
	private settlePending(): void {
		if (this.pending.length === 0) return;
		const users = this.items.filter((i) => i.kind === 'user');
		const previews: Record<string, string[]> = {};
		this.pending = this.pending.filter((p) => {
			const echo = users.slice(p.after).find((u) => u.kind === 'user' && u.text === p.text);
			if (echo && p.previews.length > 0) previews[echo.id] = p.previews;
			return !echo;
		});
		if (Object.keys(previews).length > 0) {
			this.imagePreviews = { ...this.imagePreviews, ...previews };
		}
	}

	/**
	 * The conversation has something on disk to resume. A snapshot that starts
	 * past item 0 left older history out, prompts included.
	 */
	get hasHistory(): boolean {
		return this.start > 0 || this.items.some((i) => i.kind === 'user');
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

	prompt(text: string, images: ChatImage[] = []): boolean {
		const trimmed = text.trim();
		if (!trimmed && images.length === 0) return false;
		const payload = images.map(({ mediaType, data }) => ({ mediaType, data }));
		const msg: AgentClientMsg = { t: 'prompt', text: trimmed };
		if (payload.length > 0) msg.images = payload;
		if (!this.send(msg)) return false;
		this.pending = [
			...this.pending,
			{
				id: crypto.randomUUID(),
				text: trimmed,
				previews: images.map(previewUrl),
				after: this.userTexts().length
			}
		];
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

	setModel(model: string): void {
		this.send({ t: 'model', model });
	}

	setEffort(effort: EffortLevel): void {
		this.send({ t: 'effort', effort });
	}

	/** The whole output of a tool shown as a preview; null if it's gone. */
	fullOutput(toolId: string): Promise<string | null> {
		return new Promise((resolve) => {
			if (!this.send({ t: 'output', toolId })) return resolve(null);
			this.outputWaiters[toolId] = chain(this.outputWaiters[toolId], resolve);
		});
	}

	/** The end of a background task's live output; null until the CLI writes it. */
	taskOutput(taskId: string): Promise<TaskOutput | null> {
		return new Promise((resolve) => {
			if (!this.send({ t: 'taskOutput', taskId })) return resolve(null);
			this.taskWaiters[taskId] = chain(this.taskWaiters[taskId], resolve);
		});
	}

	/**
	 * Re-attach now, e.g. when a phone wakes: after a long sleep the socket can
	 * still read OPEN while the server has let it go.
	 */
	reconnect(): void {
		if (this.disposed || this.status === 'failed' || this.status === 'exited') return;
		if (this.retryTimer) clearTimeout(this.retryTimer);
		const old = this.ws;
		this.ws = null;
		if (old) {
			old.onclose = null;
			old.close();
		}
		this.status = 'reconnecting';
		void this.connect();
	}

	dispose(): void {
		if (this.waitingOnYou) this.onNeedsYou?.(false);
		this.disposed = true;
		if (this.retryTimer) clearTimeout(this.retryTimer);
		this.ws?.close();
		this.ws = null;
	}
}

/** Two requests for the same id share one reply; both callers get it. */
function chain<T>(prev: ((v: T) => void) | undefined, next: (v: T) => void): (v: T) => void {
	return prev ? (v) => (prev(v), next(v)) : next;
}
