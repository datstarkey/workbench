import { agentClient, agentName } from '@workbench/chat-ui';
import { ControlPlaneStore } from '@workbench/control-plane-ui';
import { createHttpTransport } from '@workbench/transport';
import type {
	AgentSummary,
	ApprovalDecision,
	ClaudeAccount,
	WorkbenchSettings
} from '@workbench/types';
import { openUrl } from '@tauri-apps/plugin-opener';
import { hostOf, LS_LINKS, machineKey, normalizeUrl, SavedMachines } from './machines.svelte.ts';
import { PairingScan, type QrScanner } from './qr-scan.svelte.ts';
import { verifyServer } from './server-check.ts';
import { lsGet, lsSet } from './storage.ts';
import { baseName } from './home-format.ts';
import { ProjectPrefs } from './project-prefs.svelte.ts';
import { Drafts } from './drafts.svelte';
import { SessionNotifications } from './session-notifications.svelte';
import { ProjectReview, type ReviewFolder } from './project-review.svelte';
import type { ChatRef, ClaudeLaunch, ClaudeView, TerminalMeta } from './types.ts';

const LS_VIEW = 'wb.claudeView';
/** Home-screen refresh while the app is in front. */
const POLL_MS = 4000;

function readLinks(machineId: string): Record<string, ChatRef> {
	try {
		const parsed: unknown = JSON.parse(lsGet(machineKey(LS_LINKS, machineId)) ?? '{}');
		return parsed && typeof parsed === 'object' ? (parsed as Record<string, ChatRef>) : {};
	} catch {
		return {};
	}
}

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
	/** The server every request goes to; null while disconnected. */
	connection = $state<{ url: string; token: string } | null>(null);
	store = $state<ControlPlaneStore | null>(null);
	drafts = new Drafts('disconnected');
	projectPrefs = $state.raw(new ProjectPrefs('disconnected'));
	accounts = $state<Pick<ClaudeAccount, 'id' | 'name'>[]>([]);
	accountId = $state<string | undefined>(undefined);
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
	claudeTerminals = $state<Record<string, ChatRef>>({});
	/** A chat ↔ terminal switch is stopping one process and starting the other. */
	switching = $state(false);
	/** Why the last action failed (switch, approve, open); shown on whichever screen is up. */
	notice = $state<string | null>(null);

	readonly agents = agentClient(() => ({
		baseUrl: this.connection?.url ?? '',
		token: this.connection?.token ?? ''
	}));

	machine = $derived(this.machines.list.find((m) => m.id === this.machineId) ?? null);
	activeTerminal = $derived(this.terminals.find((t) => t.id === this.activeTerminalId) ?? null);

	private readonly pairing: PairingScan;
	/** A URL that is already a complete origin (saved, or from a pairing code): never re-normalised. */
	private exactUrl: string | null;
	/** Bumped whenever the connection goes; a response for an older connection is dropped. */
	private generation = 0;
	/** Bumped on every connect attempt; a newer attempt, a disconnect or forgetting its machine supersedes it. */
	private attempt = 0;

	constructor(scanner?: QrScanner) {
		this.pairing = new PairingScan(scanner);
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
		return this.open(this.url, this.token, this.url === this.exactUrl);
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
	private async open(rawUrl: string, rawToken: string, exact: boolean): Promise<void> {
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
			if (superseded()) return next.dispose();

			this.teardown();
			const machine = this.machines.save(base, token);
			this.url = base;
			this.exactUrl = base;
			this.token = token;
			this.connection = { url: base, token };
			this.machineId = machine.id;
			this.drafts = new Drafts(machine.id);
			this.projectPrefs = new ProjectPrefs(machine.id);
			this.controlPlane = createHttpTransport({ baseUrl: base, token });
			this.claudeTerminals = readLinks(machine.id);
			this.online = true;
			this.store = next;
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
		this.store?.dispose();
		this.store = null;
		this.connection = null;
		this.controlPlane = null;
		this.accounts = [];
		this.accountId = undefined;
		this.machineId = null;
		this.projectPrefs = new ProjectPrefs('disconnected');
		this.claudeTerminals = {};
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
	 * Keep the home screen current while the app is in front, and catch up as
	 * soon as it comes back from the lock screen. Returns a stop function.
	 */
	watch(): () => void {
		const onHome = () => !document.hidden && this.store && !this.activeChat && !this.activeTerminal;
		const timer = setInterval(() => {
			if (!onHome()) return;
			void this.refreshTerminals();
			void this.refreshChats();
		}, POLL_MS);
		const wake = () => {
			if (!document.hidden && this.store) this.refreshAll();
		};
		document.addEventListener('visibilitychange', wake);
		return () => {
			clearInterval(timer);
			document.removeEventListener('visibilitychange', wake);
		};
	}

	/** A new Claude conversation in the phone's default view. */
	startClaude = async (projectPath: string, worktreePath: string | undefined, name: string) => {
		const ref: ChatRef = {
			sessionId: crypto.randomUUID(),
			projectPath,
			worktreePath,
			name,
			...(this.accountId ? { claudeAccountId: this.accountId } : {})
		};
		if (this.defaultView === 'chat') this.openChat(ref);
		else await this.openClaudeTerminal(ref, false);
	};

	/** A new Codex conversation; always a chat (Codex has no terminal handoff here). */
	startCodex = (projectPath: string, worktreePath: string | undefined, name: string): void => {
		this.openChat({ sessionId: '', agent: 'codex', projectPath, worktreePath, name });
	};

	chatRef(chat: AgentSummary): ChatRef {
		return {
			sessionId: chat.sessionId,
			...(chat.agent === 'codex' ? { agent: 'codex' as const } : {}),
			projectPath: chat.projectPath,
			worktreePath: chat.worktreePath ?? undefined,
			name: chat.title ?? baseName(chat.worktreePath ?? chat.projectPath),
			...(chat.claudeAccountId ? { claudeAccountId: chat.claudeAccountId } : {})
		};
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
		try {
			if (sessionId) await this.agents.stop(sessionId, { end: true });
		} catch (e) {
			if (live()) this.notice = `Couldn't end the session: ${errorText(e)}`;
			return;
		}
		if (!live()) return;
		this.activeChat = null;
		await this.refreshChats();
	}

	/**
	 * Chat → terminal: stop the chat's process first (one writer per session
	 * file), then continue the conversation in a real `claude`.
	 */
	async showAsTerminal(ref: ChatRef, hasHistory: boolean): Promise<void> {
		const live = this.live();
		this.switching = true;
		this.notice = null;
		try {
			await this.agents.stop(ref.sessionId);
		} catch (e) {
			if (!live()) return;
			this.notice = `Couldn't stop the chat: ${errorText(e)}`;
			this.switching = false;
			return;
		}
		// A session id only means something on the machine it came from.
		if (!live()) return;
		this.activeChat = null;
		await this.openClaudeTerminal(ref, hasHistory);
		if (live()) this.switching = false;
	}

	/** Terminal → chat: end the terminal's `claude`, then pick the conversation up in chat. */
	async showAsChat(terminalId: string): Promise<void> {
		const ref = this.claudeTerminals[terminalId];
		if (!ref) return;
		const live = this.live();
		this.switching = true;
		this.notice = null;
		// Wait until the terminal's process group is gone: two `claude`s on one
		// session would both write its transcript.
		const stopped = await this.deleteTerminal(terminalId, true);
		if (!live()) return;
		if (stopped) {
			this.openChat(ref);
			await this.refreshTerminals();
		} else {
			this.notice = "Couldn't stop the terminal, so the chat didn't start. Try again.";
		}
		this.switching = false;
	}

	private async openClaudeTerminal(ref: ChatRef, resume: boolean): Promise<void> {
		const id = await this.createTerminal(ref.projectPath, ref.worktreePath, ref.name, {
			claudeSession: { id: ref.sessionId, resume },
			...(ref.claudeAccountId ? { claudeAccountId: ref.claudeAccountId } : {})
		});
		if (id) this.setLinks({ ...this.claudeTerminals, [id]: ref });
	}

	private setLinks(links: Record<string, ChatRef>): void {
		this.claudeTerminals = links;
		if (this.machineId) lsSet(machineKey(LS_LINKS, this.machineId), JSON.stringify(links));
	}

	async refreshTerminals(): Promise<void> {
		if (!this.store) return;
		const current = this.live();
		try {
			const res = await fetch(`${this.base}/remote/terminals`, { headers: this.authHeaders() });
			if (current()) this.online = res.ok;
			if (res.ok) {
				const data = await res.json();
				if (!current()) return;
				// Guard the {#each terminals} render: a non-array body would throw.
				this.terminals = Array.isArray(data) ? data : [];
				// eslint-disable-next-line svelte/prefer-svelte-reactivity -- local lookup only
				const live = new Set(this.terminals.map((t) => t.id));
				const links = Object.entries(this.claudeTerminals);
				if (links.some(([id]) => !live.has(id)))
					this.setLinks(Object.fromEntries(links.filter(([id]) => live.has(id))));
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

	/** `wait` returns only once its processes are gone. */
	private async deleteTerminal(id: string, wait = false): Promise<boolean> {
		const live = this.live();
		try {
			const res = await fetch(
				`${this.base}/remote/terminals/${encodeURIComponent(id)}${wait ? '?wait=true' : ''}`,
				{ method: 'DELETE', headers: this.authHeaders() }
			);
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
	}
}
