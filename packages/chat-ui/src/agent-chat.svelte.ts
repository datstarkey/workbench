import type {
	AgentClientMsg,
	AgentKind,
	AgentServerMsg,
	ApprovalDecision,
	ChatFile,
	ChatImage,
	CodexMode,
	EffortLevel,
	ElicitationAction,
	PermissionMode,
	RewindFiles,
	SlashCommand,
	StartAgentBody,
	TranscriptItem,
	TranscriptMeta
} from '@workbench/types';
import type { AgentApi } from './agent-api';
import { agentName, applyChanges, awaitsAnswer } from './chat-format';
import type { ElicitationValue } from './elicitation-form';
import { previewUrl } from './attachment-intake';

/**
 * - `starting`: launching or resuming the `claude` / `codex` process.
 * - `live`: attached; prompts go straight to the agent.
 * - `reconnecting`: the socket dropped; the agent keeps running server-side.
 * - `exited`: the process ended (crash, `/exit`, server stopped).
 * - `failed`: it could not be started at all.
 */
export type ChatStatus = 'starting' | 'live' | 'reconnecting' | 'exited' | 'failed';

/** A prompt shown at once, until the agent echoes it back as a real item. */
export interface PendingPrompt {
	id: string;
	text: string;
	/** Data URLs of attached images, shown in the bubble. */
	previews: string[];
	/** Names of attached PDFs and text files. */
	files: string[];
	/** User items already in the chat when it was sent; only later ones can echo it. */
	after: number;
}

const RECONNECT_MS = 1500;
/** How long the `@` menu's file list is reused before it's fetched again. */
const FILES_TTL_MS = 30_000;

export interface TaskOutput {
	text: string;
	/** Total size of the output so far. */
	bytes: number;
}

/** A rewind being set up: `checking` previews the file restore, `working` applies it. */
export interface RewindState {
	messageId: string;
	/** The prompt rewound to; a conversation rewind puts it back in the composer. */
	text: string;
	phase: 'checking' | 'ready' | 'working';
	/** The dry run's answer: which files a code restore would change. */
	files: RewindFiles | null;
	error: string | null;
}

type RewindReply = Extract<AgentServerMsg, { t: 'rewind' }>;

/** One chat view's connection to its Claude or Codex session on a Workbench server. */
export class AgentChat {
	readonly agent: AgentKind;
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
	/**
	 * The session's id: the one the server's start returned (a new Codex thread
	 * has none before), then any new id `/clear` moved it to. The pane follows it.
	 */
	sessionId = $state('');
	/** Slash commands for the composer's `/` menu. */
	commands = $state.raw<SlashCommand[]>([]);
	/** Previews of images sent from here, by the user item that echoed them. */
	imagePreviews = $state.raw<Record<string, string[]>>({});
	/** When each task or running tool was first seen (client clock), for timers. */
	seenAt = $state.raw<Record<string, number>>({});
	/** The open "Rewind to here" panel, if any. */
	rewind = $state.raw<RewindState | null>(null);

	private body: StartAgentBody;
	private readonly api: AgentApi;
	private ws: WebSocket | null = null;
	private retryTimer: ReturnType<typeof setTimeout> | null = null;
	private disposed = false;
	private threadStart: Promise<string> | null = null;
	/** Bumped by every connect; an older one still awaiting the server gives up. */
	private generation = 0;
	/** Called when the agent starts or stops waiting on the person (approval, question). */
	onNeedsYou: ((waiting: boolean) => void) | null = null;
	/** An `attachOnly` chat that ended was restarted here: this device now owns it. */
	onTakeOver: (() => void) | null = null;
	/** The session was ended (e.g. End on another device), so its view can close. */
	onEnded: (() => void) | null = null;
	private waitingOnYou = false;
	/** Callbacks waiting on `output` / `taskOutput` replies; not UI state, so not reactive. */
	private outputWaiters: Record<string, (text: string | null) => void> = {};
	private taskWaiters: Record<string, (out: TaskOutput | null) => void> = {};
	private fileList: { at: number; files: Promise<string[]> } | null = null;
	private rewindWaiters: Record<string, (reply: RewindReply | null) => void> = {};

	constructor(body: StartAgentBody, api: AgentApi) {
		this.body = body;
		this.api = api;
		this.agent = body.agent ?? 'claude';
		this.sessionId = body.sessionId ?? '';
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
	 * A new Codex thread has no id to make its start idempotent, so a restart
	 * or reconnect mid-start joins the one in flight, and its id is kept even
	 * when the connect that asked for it has gone stale.
	 */
	private startThread(): Promise<string> {
		this.threadStart ??= this.api
			.start({ ...this.body, sessionId: undefined })
			.then((id) => {
				this.sessionId ||= id;
				return id;
			})
			.finally(() => (this.threadStart = null));
		return this.threadStart;
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
			const sessionId = this.sessionId
				? await this.api.start({ ...this.body, sessionId: this.sessionId })
				: await this.startThread();
			if (stale()) return;
			this.sessionId = sessionId;
			url = await this.api.socketUrl(sessionId);
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
			this.settleRewinds();
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
				if (msg.ended) this.onEnded?.();
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
			case 'rewind':
				this.rewindWaiters[msg.messageId]?.(msg);
				delete this.rewindWaiters[msg.messageId];
				break;
			case 'replaced':
				// A conversation rewind restarted the process under the same id.
				this.reconnect();
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
		const waiting = this.items.some(awaitsAnswer);
		if (waiting === this.waitingOnYou) return;
		this.waitingOnYou = waiting;
		this.onNeedsYou?.(waiting);
	}

	/** Drop optimistic prompts the agent has echoed back. */
	private settlePending(): void {
		if (this.pending.length === 0) return;
		const users = this.items.filter((i) => i.kind === 'user');
		// The CLI collapses runs of spaces in a slash command's echo.
		const same = (a: string, b: string) => a.replace(/\s+/g, ' ') === b.replace(/\s+/g, ' ');
		const previews: Record<string, string[]> = {};
		this.pending = this.pending.filter((p) => {
			const echo = users.slice(p.after).find((u) => u.kind === 'user' && same(u.text, p.text));
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
			this.notice = `Not connected to ${agentName(this.agent)} yet. Try again in a moment.`;
			return false;
		}
		this.ws.send(JSON.stringify(msg));
		return true;
	}

	prompt(text: string, images: ChatImage[] = [], files: ChatFile[] = []): boolean {
		const trimmed = text.trim();
		if (!trimmed && images.length === 0 && files.length === 0) return false;
		const payload = images.map(({ mediaType, data }) => ({ mediaType, data }));
		const msg: AgentClientMsg = { t: 'prompt', text: trimmed };
		if (payload.length > 0) msg.images = payload;
		if (files.length > 0) msg.files = files;
		if (!this.send(msg)) return false;
		this.pending = [
			...this.pending,
			{
				id: crypto.randomUUID(),
				text: trimmed,
				previews: images.map(previewUrl),
				files: files.map((f) => f.name),
				after: this.userTexts().length
			}
		];
		this.busySince ??= Date.now();
		return true;
	}

	/**
	 * The session cwd's files for the composer's `@` menu, fetched at most
	 * every 30s; empty when the server can't list them.
	 */
	listFiles(): Promise<string[]> {
		const now = Date.now();
		if (!this.fileList || now - this.fileList.at > FILES_TTL_MS) {
			const { projectPath, worktreePath } = this.body;
			const files = this.api.files?.({ projectPath, worktreePath }) ?? Promise.resolve([]);
			this.fileList = { at: now, files: files.catch(() => []) };
		}
		return this.fileList.files;
	}

	approve(requestId: string, decision: ApprovalDecision, answers?: Record<string, string>): void {
		this.send({ t: 'approve', requestId, decision, ...(answers ? { answers } : {}) });
	}

	/** Answer an MCP elicitation; `content` only with `accept` on a form. */
	elicit(
		requestId: string,
		action: ElicitationAction,
		content?: Record<string, ElicitationValue>
	): void {
		this.send({ t: 'elicit', requestId, action, ...(content ? { content } : {}) });
	}

	interrupt(): void {
		this.send({ t: 'interrupt' });
	}

	setMode(mode: PermissionMode | CodexMode): void {
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

	/** Rewind is offered only for Claude, attached, idle and not waiting on an answer. */
	get canRewind(): boolean {
		return (
			this.agent === 'claude' &&
			this.status === 'live' &&
			!this.meta?.busy &&
			this.pending.length === 0 &&
			!this.items.some(awaitsAnswer)
		);
	}

	/** Open the rewind panel for a prompt and preview what a code restore would change. */
	async beginRewind(messageId: string, text: string): Promise<void> {
		if (!this.canRewind) return;
		const base: RewindState = { messageId, text, phase: 'checking', files: null, error: null };
		this.rewind = base;
		const reply = await this.requestRewind(messageId, true, false, true);
		if (this.rewind !== base) return; // cancelled or another prompt picked
		this.rewind = {
			...base,
			phase: 'ready',
			files: reply?.files ?? null,
			error: reply ? reply.error : 'Not connected.'
		};
	}

	/**
	 * Apply the open rewind. Resolves to the prompt's text after a conversation
	 * rewind (for the composer), else null. The cut tail is dropped at once;
	 * the server's restart then sends `replaced` and the re-attach confirms it.
	 */
	async confirmRewind(code: boolean, conversation: boolean): Promise<string | null> {
		const open = this.rewind;
		if (!open || open.phase !== 'ready' || (!code && !conversation)) return null;
		const working: RewindState = { ...open, phase: 'working', error: null };
		this.rewind = working;
		const reply = await this.requestRewind(open.messageId, code, conversation, false);
		if (this.rewind !== working) return null;
		const error = reply ? reply.error : 'Not connected.';
		if (error) {
			this.rewind = { ...open, error };
			return null;
		}
		this.rewind = null;
		if (!conversation) return null;
		const at = this.items.findIndex((i) => i.id === open.messageId);
		if (at >= 0) this.items = this.items.slice(0, at);
		return open.text;
	}

	cancelRewind(): void {
		this.rewind = null;
	}

	private requestRewind(
		messageId: string,
		code: boolean,
		conversation: boolean,
		dryRun: boolean
	): Promise<RewindReply | null> {
		return new Promise((resolve) => {
			if (!this.send({ t: 'rewind', messageId, code, conversation, dryRun })) return resolve(null);
			this.rewindWaiters[messageId] = chain(this.rewindWaiters[messageId], resolve);
		});
	}

	/** The socket closed: no reply is coming. */
	private settleRewinds(): void {
		const waiters = Object.values(this.rewindWaiters);
		this.rewindWaiters = {};
		for (const resolve of waiters) resolve(null);
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
