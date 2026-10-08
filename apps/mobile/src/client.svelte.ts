import { agentClient, agentName } from '@workbench/chat-ui';
import { ControlPlaneStore } from '@workbench/control-plane-ui';
import { createHttpTransport } from '@workbench/transport';
import type {
	AgentSummary,
	ApprovalDecision,
	ClaudeAccount,
	CreateServerTerminalBody,
	ServerTerminalMeta as TerminalMeta,
	WorkbenchSettings
} from '@workbench/types';
import { openUrl } from '@tauri-apps/plugin-opener';
import { untrack } from 'svelte';
import { HomeStream, type OpenEventSource } from './home-stream.ts';
import { HostUpdate } from './host-update.svelte.ts';
import { hostOf, machineKey, normalizeUrl, SavedMachines } from './machines.svelte.ts';
import { PairingScan, type QrScanner } from './qr-scan.svelte.ts';
import { verifyServer } from './server-check.ts';
import { lsGet, lsSet } from './storage.ts';
import { baseName } from './home-format.ts';
import { ProjectPrefs } from './project-prefs.svelte.ts';
import { Drafts } from './drafts.svelte';
import { SessionNotifications, type NotificationSession } from './session-notifications.svelte';
import { ProjectReview, type ReviewFolder } from './project-review.svelte';
import type { ChatRef, ClaudeView } from './types.ts';

/** Extras for a terminal that runs `claude` on a conversation (the server builds the command). */
type ClaudeLaunch = Pick<CreateServerTerminalBody, 'claudeSession' | 'claudeAccountId'>;

const LS_VIEW = 'wb.claudeView';
/** Home-screen refresh while the app is in front and the event stream is down. */
const POLL_MS = 4000;
/** A list request that takes longer is given up, and the machine shown offline. */
const REQUEST_TIMEOUT_MS = 10_000;

function errorText(e: unknown): string {
	return e instanceof Error ? e.message : String(e);
}

/** Uses the system URL handler without the opener plugin's inAppBrowser mode. */
export function openExternal(url: string): void {
	openUrl(url).catch((e) => console.warn('[mobile] open url', url, e));
}

/**
 * Phone-side connection + terminal state for the mobile app. Owns the
 * control-plane store (over HTTP) plus the persistent-terminal list and the
 * active terminal. Kept out of the component so it can be unit-tested.
 */
export class MobileClient {
	readonly machines = new SavedMachines();
	readonly notifications = new SessionNotifications();
	/** The connect form's fields (they show the last connected machine). */
	url = $state(this.machines.active?.url ?? '');
	token = $state(this.machines.active?.token ?? '');
	nickname = $state('');
	/** The server every request goes to; null while disconnected. */
	connection = $state<{ url: string; token: string } | null>(null);
	store = $state<ControlPlaneStore | null>(null);
	drafts = new Drafts('disconnected');
	projectPrefs = $state.raw(new ProjectPrefs('disconnected'));
	accounts = $state<Pick<ClaudeAccount, 'id' | 'name'>[]>([]);
	accountId = $state<string | undefined>(undefined);
	/** The connected host's version and update; null while disconnected. */
	hostUpdate = $state.raw<HostUpdate | null>(null);
	private controlPlane: ReturnType<typeof createHttpTransport> | null = null;

	setAccount(id: string): void {
		this.accountId = id || undefined;
		if (this.machineId) lsSet(machineKey('wb.account', this.machineId), id);
	}

	private async loadAccounts(): Promise<void> {
		const live = this.live();
		try {
			const settings = (await this.controlPlane?.invoke(
				'load_workbench_settings',
				undefined
			)) as WorkbenchSettings | null;
			if (!live()) return;
			this.accounts = (settings?.claudeAccounts ?? []).map(({ id, name }) => ({ id, name }));
			const saved = this.machineId ? lsGet(machineKey('wb.account', this.machineId)) : null;
			const selected = saved ?? settings?.activeClaudeAccount ?? '';
			this.accountId = this.accounts.some((a) => a.id === selected) ? selected : undefined;
		} catch {
			/* Older servers still support the default account. */
		}
	}

	review(folder: ReviewFolder): ProjectReview {
		if (!this.controlPlane) throw new Error('Connect to a machine first');
		return new ProjectReview(this.controlPlane, folder);
	}
	connecting = $state(false);
	/** The saved machine a connect is in flight to (null for one not saved yet). */
	connectingTo = $state<string | null>(null);
	connectError = $state<string | null>(null);
	/** The connected machine's id (null while disconnected). */
	machineId = $state<string | null>(null);
	/** Whether the connected machine answered the last terminal-list refresh. */
	online = $state(true);
	terminals = $state<TerminalMeta[]>([]);
	activeTerminalId = $state<string | null>(null);

	/** Chat sessions running on the server (any device's). */
	chats = $state<AgentSummary[]>([]);
	activeChat = $state<ChatRef | null>(null);
	chatScreenKey = $state(0);
	defaultView = $state<ClaudeView>(lsGet(LS_VIEW) === 'terminal' ? 'terminal' : 'chat');
	/** A Claude view switch is finding and attaching to the existing session. */
	switching = $state(false);
	/** The chat this client is ending; not UI state. */
	private ending: string | null = null;
	/** Why the last action failed (switch, approve, open); shown on whichever screen is up. */
	notice = $state<string | null>(null);

	readonly agents = agentClient(() => ({
		baseUrl: this.connection?.url ?? '',
		token: this.connection?.token ?? ''
	}));

	machine = $derived(this.machines.list.find((m) => m.id === this.machineId) ?? null);
	activeTerminal = $derived(this.terminals.find((t) => t.id === this.activeTerminalId) ?? null);
	/**
	 * Terminal id → the Claude conversation its `claude` runs, from the server:
	 * every Claude terminal lists its session, and a chat its terminal.
	 */
	terminalChats = $derived.by(() => {
		const links: Record<string, ChatRef> = {};
		for (const t of this.terminals) {
			if (t.claudeSessionId)
				links[t.id] = {
					sessionId: t.claudeSessionId,
					projectPath: t.cwd,
					name: t.name ?? baseName(t.cwd),
					attachOnly: true
				};
		}
		for (const chat of this.chats) {
			if (!chat.exited && chat.terminalId) links[chat.terminalId] = this.chatRef(chat);
		}
		return links;
	});
	/** A Claude chat and its backing terminal are one entry on Home. */
	standaloneTerminals = $derived.by(() => {
		const backing = new Set(this.chats.filter((c) => !c.exited).map((c) => c.terminalId));
		return this.terminals.filter((t) => !backing.has(t.id));
	});

	private readonly pairing: PairingScan;
	/** A URL that is already a complete origin (saved, or from a pairing code): never re-normalised. */
	private exactUrl: string | null;
	/** Bumped whenever the connection goes; a response for an older connection is dropped. */
	private generation = 0;
	/** Bumped on every connect attempt; a newer attempt, a disconnect or forgetting its machine supersedes it. */
	private attempt = 0;

	/** Whether the app is in front; follows `visibilitychange` while watching. */
	private visible = $state(!document.hidden);
	/** The server whose home lists to stream: only on Home, in front. */
	private homeServer = $derived(
		this.visible && this.store && !this.activeChat && !this.activeTerminal ? this.connection : null
	);
	private readonly homeStream: HomeStream;

	constructor(scanner?: QrScanner, openEventSource?: OpenEventSource) {
		this.pairing = new PairingScan(scanner);
		this.homeStream = new HomeStream(
			{
				agents: (list) => (this.chats = list),
				terminals: (list) => (this.terminals = list),
				status: (live) => {
					if (live) this.online = true;
				}
			},
			openEventSource
		);
		this.exactUrl = this.url || null;
	}

	/** Whether a machine was saved as active (auto-reconnect on launch). */
	get hasSavedServer(): boolean {
		return !!this.machines.active;
	}

	private authHeaders(): Record<string, string> {
		return { authorization: `Bearer ${this.connection?.token ?? ''}` };
	}

	private get base(): string {
		return this.connection?.url ?? '';
	}

	/** True while the connection it was taken on is still the current one. */
	private live(): () => boolean {
		const generation = this.generation;
		return () => generation === this.generation;
	}

	/** Connect to the form's server; on success it is saved and active. */
	connect(): Promise<void> {
		// Only hand-typed input gets a scheme and the default port added: a scanned
		// `https://box.ts.net` must not become `https://box.ts.net:4317`.
		return this.open(this.url, this.token, this.url === this.exactUrl, this.nickname);
	}

	/** Switch to a saved machine. The current connection stays until the new one has answered. */
	async switchTo(id: string): Promise<void> {
		const machine = this.machines.list.find((m) => m.id === id);
		if (machine) await this.open(machine.url, machine.token, true);
	}

	/**
	 * Verify a server (health + token), then replace the current connection with
	 * it. A failure leaves the current connection as it was.
	 */
	private async open(
		rawUrl: string,
		rawToken: string,
		exact: boolean,
		nickname = ''
	): Promise<void> {
		const attempt = ++this.attempt;
		const superseded = () => attempt !== this.attempt;
		const base = exact ? rawUrl : normalizeUrl(rawUrl);
		const token = rawToken.trim();
		const saved = this.machines.find(base, token);
		this.connectingTo = saved?.id ?? null;
		this.connecting = true;
		this.connectError = null;
		try {
			if (!base) throw new Error('enter a server address');
			// Every Workbench server requires a token (see Settings → Server mode).
			if (!token) throw new Error('enter the server token');
			await verifyServer(base, token);
			if (superseded()) return;
			const next = new ControlPlaneStore(createHttpTransport({ baseUrl: base, token }));
			await next.refresh();
			if (superseded()) return;

			this.teardown();
			const machine = this.machines.save(base, token, nickname);
			this.nickname = '';
			this.url = base;
			this.exactUrl = base;
			this.token = token;
			this.connection = { url: base, token };
			this.machineId = machine.id;
			this.drafts = new Drafts(machine.id);
			this.projectPrefs = new ProjectPrefs(machine.id);
			const controlPlane = createHttpTransport({ baseUrl: base, token });
			this.controlPlane = controlPlane;
			this.online = true;
			this.store = next;
			this.hostUpdate = new HostUpdate(controlPlane);
			void this.hostUpdate.check();
			await Promise.all([this.refreshTerminals(), this.refreshChats(), this.loadAccounts()]);
		} catch (e) {
			if (superseded()) return;
			if (this.store)
				this.notice = `Couldn't switch to ${saved?.name ?? hostOf(base)}: ${errorText(e)}`;
			else this.connectError = saved ? `${saved.name}: ${errorText(e)}` : errorText(e);
		} finally {
			if (!superseded()) this.endConnecting();
		}
	}

	private endConnecting(): void {
		this.connecting = false;
		this.connectingTo = null;
	}

	/** Leave the current machine for an empty connect form (scan or type a new one). */
	addMachine(): void {
		this.disconnect();
		this.url = '';
		this.token = '';
		this.nickname = '';
		this.exactUrl = null;
	}

	/** Forget a saved machine; forgetting the connected one returns to the connect form. */
	forget(id: string): void {
		if (id === this.connectingTo) {
			this.attempt++;
			this.endConnecting();
		}
		const current = id === this.machineId;
		this.machines.remove(id);
		if (current) this.addMachine();
	}

	/** Scan the desktop's pairing QR code, then connect to (and save) that machine. */
	async scanAndConnect(): Promise<void> {
		this.connectError = null;
		try {
			const pairing = await this.pairing.scan();
			if (!pairing) return;
			this.url = pairing.url;
			this.exactUrl = pairing.url;
			this.token = pairing.token;
			await this.connect();
		} catch (e) {
			this.connectError = errorText(e);
		}
	}

	get scanning(): boolean {
		return this.pairing.scanning;
	}

	/** Overlay Cancel / Android back. */
	cancelScan = (): Promise<void> => this.pairing.cancel();

	/** Close the connection (and any connect in flight). */
	disconnect(): void {
		this.attempt++;
		this.endConnecting();
		this.connectError = null;
		this.teardown();
	}

	/** Drop the connection: every screen of it goes, and late responses for it are dropped. */
	private teardown(): void {
		this.generation++;
		this.store = null;
		this.connection = null;
		this.controlPlane = null;
		this.hostUpdate = null;
		this.accounts = [];
		this.accountId = undefined;
		this.machineId = null;
		this.projectPrefs = new ProjectPrefs('disconnected');
		this.terminals = [];
		this.chats = [];
		this.activeTerminalId = null;
		this.activeChat = null;
		this.switching = false;
		this.notice = null;
	}

	setDefaultView(view: ClaudeView): void {
		this.defaultView = view;
		lsSet(LS_VIEW, view);
	}

	async refreshChats(): Promise<void> {
		if (!this.store) return;
		const live = this.live();
		try {
			const chats = await this.agents.list();
			if (live()) this.chats = chats;
		} catch {
			/* keep the last list */
		}
	}

	/**
	 * Keep the home screen current while the app is in front: streamed from
	 * `/events/home`, polled while the stream is down (or the host predates it),
	 * and caught up as soon as it comes back from the lock screen. Returns a
	 * stop function.
	 */
	watch(): () => void {
		// Opening a connection is an external side effect: an effect is the right tool.
		const stopStream = $effect.root(() => {
			$effect(() => {
				const server = this.homeServer;
				untrack(() => this.homeStream.follow(server));
			});
		});
		let polling = false;
		const timer = setInterval(() => {
			// The host is restarting into its update: its first answer ends "Updating host…".
			if (!document.hidden && this.hostUpdate?.updating) void this.hostUpdate.check();
			// One round at a time: a stalled host must not pile requests up.
			if (!this.homeServer || this.homeStream.live || polling) return;
			polling = true;
			void Promise.allSettled([this.refreshTerminals(), this.refreshChats()]).then(
				() => (polling = false)
			);
		}, POLL_MS);
		const wake = () => {
			this.visible = !document.hidden;
			if (this.visible && this.store) this.refreshAll();
		};
		document.addEventListener('visibilitychange', wake);
		return () => {
			clearInterval(timer);
			document.removeEventListener('visibilitychange', wake);
			stopStream();
			this.homeStream.follow(null);
		};
	}

	/** A new Claude conversation in the phone's default view. */
	startClaude = (projectPath: string, worktreePath: string | undefined, name: string) =>
		this.openClaude({
			sessionId: crypto.randomUUID(),
			projectPath,
			worktreePath,
			name,
			...(this.accountId ? { claudeAccountId: this.accountId } : {})
		});

	/** A Claude conversation, new or past (the server resumes one on disk), in the default view. */
	async openClaude(ref: ChatRef): Promise<void> {
		if (this.defaultView === 'chat') this.openChat(ref);
		else await this.openClaudeTerminal(ref);
	}

	/** A new Codex conversation; always a chat (Codex has no terminal handoff here). */
	startCodex = (projectPath: string, worktreePath: string | undefined, name: string): void => {
		this.openChat({ sessionId: '', agent: 'codex', projectPath, worktreePath, name });
	};

	chatRef(chat: NotificationSession): ChatRef {
		return {
			sessionId: chat.sessionId,
			attachOnly: true,
			...(chat.agent === 'codex' ? { agent: 'codex' as const } : {}),
			projectPath: chat.projectPath,
			worktreePath: chat.worktreePath ?? undefined,
			name: chat.title ?? baseName(chat.worktreePath ?? chat.projectPath),
			...(chat.claudeAccountId ? { claudeAccountId: chat.claudeAccountId } : {})
		};
	}

	/** A terminal notification attaches its existing process, never creates a Codex chat. */
	async openNotification(chat: NotificationSession): Promise<void> {
		if (!chat.terminalOnly) {
			this.openChat(this.chatRef(chat));
			return;
		}
		const live = this.live();
		await this.refreshTerminals();
		if (!live()) return;
		if (chat.terminalId && this.terminals.some((t) => t.id === chat.terminalId && t.alive)) {
			this.selectTerminal(chat.terminalId);
		} else {
			this.activeChat = null;
			this.activeTerminalId = null;
			this.notice = 'This Codex terminal is available on the desktop.';
		}
	}

	openChat(ref: ChatRef): void {
		this.notice = null;
		this.activeTerminalId = null;
		this.chatScreenKey++;
		this.activeChat = ref;
	}

	/** Update the screen's reference without remounting it when Codex starts or /clear re-keys. */
	updateChatId(id: string): void {
		if (this.activeChat && id) this.activeChat = { ...this.activeChat, sessionId: id };
	}

	/** The open chat was ended on another device: leave it. An End from here leaves by itself. */
	chatEnded = (sessionId: string): void => {
		if (sessionId !== this.ending) this.closeChat();
	};

	/** Arrow field — the chat view's Back. The session keeps running on the server. */
	closeChat = (): void => {
		this.activeChat = null;
		void this.refreshChats();
	};

	/** Answer an approval from the home screen, without opening the chat. */
	async answer(sessionId: string, requestId: string, decision: ApprovalDecision): Promise<void> {
		const live = this.live();
		const agent = this.chats.find((c) => c.sessionId === sessionId)?.agent;
		this.notice = null;
		try {
			await this.agents.send(sessionId, { t: 'approve', requestId, decision });
		} catch (e) {
			if (live()) this.notice = `Couldn't answer ${agentName(agent)}: ${errorText(e)}`;
		}
		if (live()) await this.refreshChats();
	}

	/** End a chat session's process and leave its screen; the conversation stays on disk. */
	async endChat(sessionId: string): Promise<void> {
		const live = this.live();
		// A Codex chat that never got a thread id has nothing running to stop.
		this.notice = null;
		this.ending = sessionId;
		try {
			if (sessionId) await this.agents.stop(sessionId, { end: true });
		} catch (e) {
			if (live()) this.notice = `Couldn't end the session: ${errorText(e)}`;
			return;
		} finally {
			this.ending = null;
		}
		if (!live()) return;
		this.activeChat = null;
		await Promise.all([this.refreshChats(), this.refreshTerminals()]);
	}

	/** Claude chat → terminal: show the PTY the same process already runs in. */
	async showAsTerminal(ref: ChatRef): Promise<void> {
		if (this.switching || ref.agent === 'codex') return;
		const live = this.live();
		const screenKey = this.chatScreenKey;
		this.switching = true;
		this.notice = null;
		await Promise.all([this.refreshTerminals(), this.refreshChats()]);
		if (!live()) return;
		if (!this.activeChat || this.chatScreenKey !== screenKey) {
			this.switching = false;
			return;
		}
		const terminal = this.terminals.find((t) => {
			const chat = this.terminalChats[t.id];
			return (
				t.alive &&
				chat &&
				(chat.sessionId === ref.sessionId ||
					this.chats.some((c) => c.terminalId === t.id && c.previousIds.includes(ref.sessionId)))
			);
		});
		if (terminal) this.selectTerminal(terminal.id);
		else this.notice = 'This chat has no running terminal to show. Restart the session in Chat.';
		this.switching = false;
	}

	/** Terminal → Claude chat: attach to its plugin, never launch a second Claude. */
	async showAsChat(terminalId: string): Promise<void> {
		if (this.switching) return;
		const live = this.live();
		const fromTerminal = this.activeTerminalId;
		this.switching = true;
		this.notice = null;
		await this.refreshChats();
		if (!live()) return;
		if (this.activeTerminalId !== fromTerminal) {
			this.switching = false;
			return;
		}
		const ref = this.terminalChats[terminalId];
		try {
			if (!ref) throw new Error('This terminal has no Claude session to attach to.');
			const sessionId = await this.agents.start({
				projectPath: ref.projectPath,
				worktreePath: ref.worktreePath,
				sessionId: ref.sessionId,
				claudeAccountId: ref.claudeAccountId,
				attachOnly: true
			});
			if (!live()) return;
			if (this.activeTerminalId !== fromTerminal) {
				this.switching = false;
				return;
			}
			this.openChat({ ...ref, sessionId, attachOnly: true });
		} catch (e) {
			if (!live()) return;
			if (this.activeTerminalId !== fromTerminal) {
				this.switching = false;
				return;
			}
			this.notice =
				(e as { status?: number }).status === 404
					? 'Claude has not connected to Chat. Complete any login or trust prompt in its terminal, then try again.'
					: `Couldn't open Chat: ${errorText(e)}`;
		}
		this.switching = false;
	}

	private async openClaudeTerminal(ref: ChatRef): Promise<void> {
		await this.createTerminal(ref.projectPath, ref.worktreePath, ref.name, {
			claudeSession: { id: ref.sessionId },
			...(ref.claudeAccountId ? { claudeAccountId: ref.claudeAccountId } : {})
		});
	}

	async refreshTerminals(): Promise<void> {
		if (!this.store) return;
		const current = this.live();
		try {
			const res = await fetch(`${this.base}/remote/terminals`, {
				headers: this.authHeaders(),
				signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS)
			});
			if (current()) this.online = res.ok;
			if (res.ok) {
				const data = await res.json();
				if (!current()) return;
				// Guard the {#each terminals} render: a non-array body would throw.
				this.terminals = Array.isArray(data) ? data : [];
			}
		} catch {
			if (current()) this.online = false;
		}
	}

	/** Open a terminal and show it; resolves to its id, or null if it failed. */
	createTerminal = async (
		projectPath: string,
		worktreePath: string | undefined,
		name: string,
		claude?: ClaudeLaunch
	): Promise<string | null> => {
		if (!this.store) return null;
		this.notice = null;
		const live = this.live();
		try {
			const res = await fetch(`${this.base}/remote/terminals`, {
				method: 'POST',
				headers: { 'content-type': 'application/json', ...this.authHeaders() },
				body: JSON.stringify({ projectPath, worktreePath, name, ...claude, cols: 80, rows: 24 })
			});
			if (!res.ok) {
				const reason = await res
					.json()
					.then((j: { error?: string }) => j.error)
					.catch(() => undefined);
				throw new Error(reason || `the server returned ${res.status}`);
			}
			const meta: TerminalMeta = await res.json();
			if (!live()) return null;
			// Show the new terminal immediately AND keep it after refreshTerminals()
			// reconciles — otherwise the refresh overwrites `terminals` with a server
			// list that hasn't surfaced the new id yet, the $derived activeTerminal goes
			// null, and the view never opens.
			const ensureVisible = () => {
				if (!this.terminals.some((t) => t.id === meta.id)) {
					this.terminals = [...this.terminals, meta];
				}
			};
			ensureVisible();
			this.activeChat = null;
			this.activeTerminalId = meta.id;
			await this.refreshTerminals();
			if (!live()) return null;
			ensureVisible();
			return meta.id;
		} catch (e) {
			if (live()) this.notice = `Couldn't open a terminal: ${errorText(e)}`;
			return null;
		}
	};

	private async deleteTerminal(id: string): Promise<boolean> {
		const live = this.live();
		try {
			const res = await fetch(`${this.base}/remote/terminals/${encodeURIComponent(id)}`, {
				method: 'DELETE',
				headers: this.authHeaders()
			});
			if (!res.ok) return false;
		} catch {
			return false;
		}
		if (!live()) return false;
		if (this.activeTerminalId === id) this.activeTerminalId = null;
		return true;
	}

	async killTerminal(id: string): Promise<void> {
		const live = this.live();
		this.notice = null;
		const stopped = await this.deleteTerminal(id);
		if (!live()) return;
		if (!stopped) {
			this.notice = "Couldn't close the terminal. It may still be running; try again.";
			return;
		}
		if (this.activeTerminalId === id) this.activeTerminalId = null;
		await this.refreshTerminals();
	}

	selectTerminal(id: string): void {
		this.notice = null;
		this.activeChat = null;
		this.activeTerminalId = id;
	}

	/** Arrow field — passed as the Terminal view's onClose callback. */
	closeTerminal = (): void => {
		this.activeTerminalId = null;
		void this.refreshTerminals();
	};

	/** Reload the control plane, the terminal list and the chat list. */
	refreshAll(): void {
		void this.store?.refresh();
		void this.refreshTerminals();
		void this.refreshChats();
		void this.hostUpdate?.check();
	}
}
