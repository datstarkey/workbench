import type {
	AgentClientMsg,
	CodexAction,
	ChatArtifact,
	AgentKind,
	AgentServerMsg,
	ApprovalDecision,
	CachePolicy,
	ChatFile,
	ChatImage,
	CodexMode,
	EffortLevel,
	ElicitationAction,
	PermissionMode,
	RewindFiles,
	SlashCommand,
	StartAgentBody,
	TaskTranscript,
	TranscriptItem,
	TranscriptMeta
} from '@workbench/types';
import { NeedsTrustError, type AgentApi } from './agent-api';
import {
	activity,
	agentName,
	applyChanges,
	awaitsAnswer,
	chatTitle,
	isRunning,
	latestTodos
} from './chat-format';
import type { ElicitationValue } from './elicitation-form';
import { previewUrl } from './attachment-intake';
import { chatArtifacts } from './artifacts';
import { ChatDraft } from './chat-draft.svelte';

/**
 * - `starting`: launching or resuming the `claude` / `codex` process.
 * - `live`: attached; prompts go straight to the agent.
 * - `reconnecting`: the socket dropped; the agent keeps running server-side.
 * - `exited`: the process ended (crash, `/exit`, server stopped).
 * - `trust`: Claude Code waits for its folder to be trusted ({@link AgentChat.trustPath}).
 * - `failed`: it could not be started at all.
 */
export type ChatStatus = 'starting' | 'live' | 'reconnecting' | 'exited' | 'trust' | 'failed';

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
	/** Absolute index the next item had when it was sent. */
	at: number;
}

const RECONNECT_MS = 1500;
/** Hidden this long (a sleeping phone or laptop), the socket may be dead while it still reads open. */
const WAKE_RECONNECT_MS = 10_000;
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
	/** The folder Claude Code asks to trust while `status` is `trust`. */
	trustPath = $state<string | null>(null);
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
	delivery = $state<'steer' | 'queue'>('steer');
	historyItems = $state.raw<TranscriptItem[]>([]);
	private historyCursor = $state<string | null | undefined>(undefined);
	private loadingHistory = false;
	onCodexEvent: ((method: string, params: Record<string, unknown>) => void) | null = null;
	private controls = new Map<
		string,
		{
			resolve: (v: unknown) => void;
			reject: (e: Error) => void;
			timer: ReturnType<typeof setTimeout>;
		}
	>();
	private artifactWaiters = new Map<string, (v: ChatArtifact[]) => void>();
	/** The open "Rewind to here" panel, if any. */
	rewind = $state.raw<RewindState | null>(null);
	/** What the server does as the prompt cache nears expiry (Claude only). */
	cachePolicy = $state.raw<CachePolicy>({ compactOnExpiry: false });

	readonly live = $derived(this.status === 'live');
	/** What the agent is doing now, or the request it waits on. */
	readonly now = $derived(activity(this.items, this.meta));
	/** The oldest approval, question or elicitation still waiting on the person. */
	readonly waiting = $derived(this.items.find(awaitsAnswer) ?? null);
	readonly tasks = $derived(this.meta?.tasks ?? []);
	readonly runningTasks = $derived(this.tasks.filter(isRunning).length);
	/** Stop also ends a Claude chat's background agents (a forked skill's), idle or not. */
	get stoppable(): boolean {
		return (
			this.live &&
			(Boolean(this.meta?.busy) ||
				(this.agent === 'claude' &&
					this.tasks.some((t) => t.kind === 'agent' && t.background && isRunning(t))))
		);
	}
	readonly todos = $derived(latestTodos(this.items));
	readonly artifactList = $derived(chatArtifacts(this.meta?.artifacts));

	private body: StartAgentBody;
	private readonly api: AgentApi;
	private ws: WebSocket | null = null;
	private retryTimer: ReturnType<typeof setTimeout> | null = null;
	private disposed = false;
	private threadStart: Promise<string> | null = null;
	/** Bumped by every connect; an older one still awaiting the server gives up. */
	private generation = 0;
	/** An `attachOnly` chat that ended was restarted here: this device now owns it. */
	onTakeOver: (() => void) | null = null;
	private echoedUsers = new Set<string>();
	/** The session was ended (e.g. End on another device), so its view can close. */
	onEnded: (() => void) | null = null;
	/** The server terminal this Claude chat's `claude` runs in, once started. */
	onTerminal: ((terminalId: string) => void) | null = null;
	/** Callbacks waiting on `output` / `taskOutput` replies; not UI state, so not reactive. */
	private outputWaiters: Record<string, (text: string | null) => void> = {};
	private taskWaiters: Record<string, (out: TaskOutput | null) => void> = {};
	private fileList: { at: number; files: Promise<string[]> } | null = null;
	private rewindWaiters: Record<string, (reply: RewindReply | null) => void> = {};
	/** Prompts sent from here; the transcript scrolls back to the newest on each. */
	sends = $state(0);
	readonly draft: ChatDraft;
	private hiddenAt = 0;
	private readonly reconnectOnWake: boolean;

	/**
	 * `draft`: hosts that keep the composer beyond this chat pass their own.
	 * `reconnectOnWake`: re-attach when the page shows again after a long sleep
	 * (a phone), since the socket can read open after the server let it go.
	 */
	constructor(
		body: StartAgentBody,
		api: AgentApi,
		opts: { draft?: ChatDraft; reconnectOnWake?: boolean } = {}
	) {
		this.body = body;
		this.api = api;
		this.agent = body.agent ?? 'claude';
		this.sessionId = body.sessionId ?? '';
		this.draft = opts.draft ?? new ChatDraft();
		this.reconnectOnWake = !!opts.reconnectOnWake && typeof document !== 'undefined';
		if (this.reconnectOnWake) {
			if (document.hidden) this.hiddenAt = Date.now();
			document.addEventListener('visibilitychange', this.onVisibility);
		}
		void this.open();
	}

	private onVisibility = () => {
		if (document.hidden) this.hiddenAt = Date.now();
		else if (Date.now() - this.hiddenAt > WAKE_RECONNECT_MS) this.reconnect();
	};

	/**
	 * Start (or resume) the process, then attach. Also the "Restart" action: an
	 * `attachOnly` chat re-attaches while it runs, and once ended it starts here.
	 */
	open(): Promise<void> {
		if (this.body.attachOnly && (this.status === 'exited' || this.status === 'failed')) {
			this.body = { ...this.body, attachOnly: false };
			this.onTakeOver?.();
		}
		if (this.retryTimer) clearTimeout(this.retryTimer);
		this.ws?.close(); // a Restart must not leave the old socket behind
		this.ws = null;
		this.status = 'starting';
		this.error = null;
		return this.connect();
	}

	/** The person trusted the folder: start again, answering Claude Code's dialog. */
	trustFolder(): Promise<void> {
		this.body = { ...this.body, trustFolder: true };
		this.trustPath = null;
		return this.open();
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
			const terminal = this.api.terminalId?.(sessionId);
			if (terminal) this.onTerminal?.(terminal);
			url = await this.api.socketUrl(sessionId);
		} catch (e) {
			if (stale()) return;
			if (e instanceof NeedsTrustError) {
				this.status = 'trust';
				this.trustPath = e.path;
				return;
			}
			// Waking phones lose the network for a moment; keep retrying rather than give up,
			// unless a joined session is gone (it exited, or someone ended it).
			const { status, ended } = e as { status?: number; ended?: boolean };
			const gone = this.body.attachOnly && status === 404;
			if (this.status === 'reconnecting' && !gone) return this.scheduleReconnect();
			this.status = this.status === 'reconnecting' ? 'exited' : 'failed';
			this.error = e instanceof Error ? e.message : String(e);
			if (gone && ended) this.onEnded?.();
			return;
		}
		if (stale()) return;
		const ws = new WebSocket(url);
		this.ws = ws;
		ws.onmessage = (event) => {
			if (this.ws !== ws) return; // a frame still arriving after a Restart closed it
			try {
				this.receive(JSON.parse(String(event.data)) as AgentServerMsg);
			} catch (e) {
				console.warn('[AgentChat] bad frame', e);
			}
		};
		ws.onclose = () => {
			if (this.ws !== ws) return; // replaced by a Restart
			this.ws = null;
			this.rejectControls('Connection lost');
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
			case 'codexResult': {
				const pending = this.controls.get(msg.requestId);
				if (!pending) break;
				clearTimeout(pending.timer);
				this.controls.delete(msg.requestId);
				if (msg.error) {
					this.notice = msg.error;
					pending.reject(new Error(msg.error));
				} else pending.resolve(msg.result);
				break;
			}
			case 'codexEvent':
				this.onCodexEvent?.(msg.method, msg.params);
				break;
			case 'artifacts':
				this.artifactWaiters.get(msg.id)?.(msg.content);
				this.artifactWaiters.delete(msg.id);
				break;
			case 'snapshot':
				// `/clear` re-keys to a new, empty transcript. The CLI never echoes
				// the `/clear` itself; prompts queued after it echo in the new one.
				if (msg.sessionId !== this.sessionId) {
					this.historyItems = [];
					this.historyCursor = undefined;
					this.echoedUsers.clear();
					const clear = this.pending.findIndex((p) => /^\/clear(\s|$)/.test(p.text));
					this.pending = this.pending.slice(clear + 1).map((p) => ({ ...p, after: 0, at: 0 }));
				}
				this.sessionId = msg.sessionId;
				this.start = msg.start;
				this.items = msg.items;
				this.commands = msg.commands;
				this.cachePolicy = msg.cachePolicy ?? { compactOnExpiry: false };
				this.setMeta(msg.meta);
				this.status = msg.exited ? 'exited' : 'live';
				this.settlePending();
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
				break;
			case 'exit':
				this.rejectControls(`The ${agentName(this.agent)} session ended`);
				this.status = 'exited';
				this.error = msg.message;
				this.pending = [];
				this.busySince = null;
				this.ws?.close();
				if (msg.ended) this.onEnded?.();
				break;
			case 'error':
				this.notice = msg.message;
				break;
			case 'commands':
				this.commands = msg.commands;
				break;
			case 'cachePolicy':
				this.cachePolicy = msg.policy;
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
				this.rejectControls('The connection was revoked');
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

	/** Drop optimistic prompts the agent has echoed back. */
	private settlePending(): void {
		if (this.pending.length === 0) return;
		const users = this.items.filter((i) => i.kind === 'user');
		// The CLI collapses runs of spaces in a slash command's echo, and echoes
		// an alias like `/design consent` as `/design-consent`.
		const norm = (s: string) => s.replace(/[\s-]+/g, ' ');
		// A terminal session's prompt comes back with its attachments as `@path` mentions.
		const same = (echo: string, sent: string) =>
			norm(echo) === norm(sent) || norm(echo).startsWith(`${norm(sent)} @`.trimStart());
		// A command the CLI can't run headless (`/design-login`) is never echoed,
		// only answered with a notice.
		const idle = !this.meta?.busy;
		const noticeSince = (at: number) =>
			this.items.some((i, k) => i.kind === 'notice' && this.start + k >= at);
		const previews: Record<string, string[]> = {};
		const matched = this.echoedUsers;
		const available = new Set(users.map((user) => user.id));
		for (const id of matched) if (!available.has(id)) matched.delete(id);
		this.pending = this.pending.filter((p) => {
			const echo = users
				.slice(p.after)
				.find((u) => u.kind === 'user' && !matched.has(u.id) && same(u.text, p.text));
			if (echo) matched.add(echo.id);
			if (echo && p.previews.length > 0) previews[echo.id] = p.previews;
			return !echo && !(idle && p.text.startsWith('/') && noticeSince(p.at));
		});
		if (Object.keys(previews).length > 0) {
			this.imagePreviews = { ...this.imagePreviews, ...previews };
		}
	}

	get title(): string {
		return chatTitle(this.items, this.meta, this.agent);
	}

	/**
	 * The conversation has something on disk to resume. A snapshot that starts
	 * past item 0 left older history out, prompts included.
	 */
	get hasHistory(): boolean {
		return (
			this.start > 0 ||
			this.hasOlderHistory ||
			this.historyItems.length > 0 ||
			this.items.some((i) => i.kind === 'user')
		);
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

	prompt(
		text: string,
		images: ChatImage[] = [],
		files: ChatFile[] = []
	): boolean | Promise<boolean> {
		const model = this.meta?.models.find(
			(m) =>
				m.value === (this.meta?.modelChoice ?? this.meta?.model) ||
				m.resolvedModel === this.meta?.model
		);
		if (
			this.agent === 'codex' &&
			images.length &&
			model?.inputModalities?.length &&
			!model.inputModalities.includes('image')
		) {
			this.notice =
				'The selected Codex model does not accept images. Choose a model with image support.';
			return false;
		}
		const trimmed = text.trim();
		if (!trimmed && images.length === 0 && files.length === 0) return false;
		if (
			this.agent === 'codex' &&
			this.delivery === 'queue' &&
			(this.meta?.busy || this.meta?.codex?.queuePaused)
		) {
			return this.codexAction('queueAdd', {
				text: trimmed,
				images: images.map(({ mediaType, data }) => ({ mediaType, data })),
				files
			}).then(
				() => {
					this.sends++;
					return true;
				},
				() => false
			);
		}
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
				after: this.userTexts().length,
				at: this.start + this.items.length
			}
		];
		this.busySince ??= Date.now();
		this.sends++;
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
	): Promise<void> {
		if (this.agent === 'codex')
			return this.codexAction('elicitation', {
				id: requestId,
				choice: action,
				...(content ? { content } : {})
			}).then(() => {});
		if (!this.send({ t: 'elicit', requestId, action, ...(content ? { content } : {}) }))
			return Promise.reject(new Error(this.notice ?? 'Disconnected'));
		return Promise.resolve();
	}

	interrupt(): void {
		this.send({ t: 'interrupt' });
	}

	/** Restart the session's `claude`, a stuck turn included; the server's `replaced` re-attaches. */
	restart(): void {
		this.send({ t: 'restart' });
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

	codexAction(action: CodexAction, params: Record<string, unknown> = {}): Promise<unknown> {
		const requestId = crypto.randomUUID();
		return new Promise((resolve, reject) => {
			const timer = setTimeout(() => {
				this.controls.delete(requestId);
				const error = new Error('Codex action timed out');
				this.notice = error.message;
				reject(error);
			}, 35000);
			this.controls.set(requestId, { resolve, reject, timer });
			if (!this.send({ t: 'codex', requestId, action, params })) {
				clearTimeout(timer);
				this.controls.delete(requestId);
				reject(new Error(this.notice ?? 'Disconnected'));
			}
		});
	}
	get hasOlderHistory(): boolean {
		return this.historyCursor === undefined
			? !!this.meta?.codex?.hasOlderHistory
			: this.historyCursor !== null;
	}
	async loadOlder(): Promise<void> {
		if (this.loadingHistory) return;
		if (
			this.historyItems.length >= 2000 ||
			JSON.stringify(this.historyItems).length >= 16 * 1024 * 1024
		) {
			throw new Error(
				'Earlier history display limit reached. Open this thread in the Codex terminal to read more.'
			);
		}
		this.loadingHistory = true;
		const sessionId = this.sessionId;
		try {
			const result = (await this.codexAction(
				'history',
				this.historyCursor === undefined ? {} : { cursor: this.historyCursor }
			)) as { items: TranscriptItem[]; nextCursor: string | null };
			if (sessionId !== this.sessionId) return;
			this.historyItems = [...result.items, ...this.historyItems];
			this.historyCursor = result.nextCursor ?? null;
		} finally {
			this.loadingHistory = false;
		}
	}

	artifacts(id: string): Promise<ChatArtifact[]> {
		return new Promise((resolve) => {
			const timer = setTimeout(() => {
				this.artifactWaiters.delete(id);
				resolve([]);
			}, 30000);
			this.artifactWaiters.set(id, (v) => {
				clearTimeout(timer);
				resolve(v);
			});
			if (!this.send({ t: 'artifacts', id })) {
				clearTimeout(timer);
				this.artifactWaiters.delete(id);
				resolve([]);
			}
		});
	}
	private rejectControls(message: string): void {
		for (const pending of this.controls.values()) {
			clearTimeout(pending.timer);
			pending.reject(new Error(message));
		}
		this.controls.clear();
		for (const waiter of this.artifactWaiters.values()) waiter([]);
		this.artifactWaiters.clear();
		Object.values(this.outputWaiters).forEach((v) => v(null));
		this.outputWaiters = {};
		Object.values(this.taskWaiters).forEach((v) => v(null));
		this.taskWaiters = {};
	}
	/** Refresh the prompt cache now with a hidden keep-alive turn. */
	pingCache(): void {
		this.send({ t: 'cachePing' });
	}

	setCachePolicy(policy: CachePolicy): void {
		if (this.send({ t: 'cachePolicy', policy })) this.cachePolicy = policy;
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
		if (this.agent === 'codex')
			return this.codexAction('inspect', { section: 'task', threadId: taskId })
				.then((result) => {
					const entries = (
						result as {
							data: {
								item: { text?: string; aggregatedOutput?: string; content?: { text?: string }[] };
							}[];
						}
					).data;
					const text = [...(entries ?? [])]
						.reverse()
						.flatMap(({ item }) =>
							item.text
								? [item.text]
								: item.aggregatedOutput
									? [item.aggregatedOutput]
									: (item.content ?? []).flatMap((c) => (c.text ? [c.text] : []))
						)
						.join('\n\n');
					return { text: text.slice(-65536), bytes: new TextEncoder().encode(text).length };
				})
				.catch(() => null);
		return new Promise((resolve) => {
			if (!this.send({ t: 'taskOutput', taskId })) return resolve(null);
			this.taskWaiters[taskId] = chain(this.taskWaiters[taskId], resolve);
		});
	}

	/** A subagent's own conversation; null until it exists or when it can't be read. */
	async taskTranscript(taskId: string): Promise<TaskTranscript | null> {
		if (!this.sessionId || !this.api.taskTranscript) return null;
		return this.api.taskTranscript(this.sessionId, taskId).catch(() => null);
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
		if (this.disposed || ['failed', 'exited', 'trust'].includes(this.status)) return;
		if (this.retryTimer) clearTimeout(this.retryTimer);
		const old = this.ws;
		this.ws = null;
		if (old) {
			old.onclose = null;
			old.close();
			// No reply comes over the old socket now.
			this.rejectControls('Connection lost');
			this.settleRewinds();
		}
		this.status = 'reconnecting';
		void this.connect();
	}

	dispose(): void {
		this.rejectControls('Chat closed');
		this.disposed = true;
		if (this.reconnectOnWake) document.removeEventListener('visibilitychange', this.onVisibility);
		if (this.retryTimer) clearTimeout(this.retryTimer);
		this.ws?.close();
		this.ws = null;
	}
}

/** Two requests for the same id share one reply; both callers get it. */
function chain<T>(prev: ((v: T) => void) | undefined, next: (v: T) => void): (v: T) => void {
	return prev ? (v) => (prev(v), next(v)) : next;
}
