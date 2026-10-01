import { agentClient } from '@workbench/chat-ui';
import { ControlPlaneStore } from '@workbench/control-plane-ui';
import { createHttpTransport } from '@workbench/transport';
import type { AgentSummary, ApprovalDecision } from '@workbench/types';
import { openUrl } from '@tauri-apps/plugin-opener';
import { LS_LINKS, machineKey, normalizeUrl, SavedMachines } from './machines.svelte.ts';
import { PairingScan, type QrScanner } from './qr-scan.svelte.ts';
import { lsGet, lsSet } from './storage.ts';

export type TerminalMeta = {
	id: string;
	name?: string;
	cwd: string;
	createdAt: number;
	alive: boolean;
};

/** How a new Claude session opens on this phone. */
export type ClaudeView = 'chat' | 'terminal';

/** A Claude conversation, shown as chat or as a terminal running `claude`. */
export interface ChatRef {
	sessionId: string;
	projectPath: string;
	worktreePath?: string;
	name: string;
	/** The Claude account it belongs to; absent is the default login. */
	claudeAccountId?: string;
}

/** Extras for a terminal that runs `claude` on a conversation (the server builds the command). */
interface ClaudeLaunch {
	claudeSession: { id: string; resume: boolean };
	claudeAccountId?: string;
}

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

export function baseName(path: string): string {
	return (
		path
			.replace(/[\\/]+$/, '')
			.split(/[\\/]/)
			.pop() || path
	);
}

/** Opens `url` in the system browser; a failure is logged, never thrown. */
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
	/** The connect form's fields, and the connected machine's address and token. */
	url = $state(this.machines.active?.url ?? '');
	token = $state(this.machines.active?.token ?? '');
	store = $state<ControlPlaneStore | null>(null);
	connecting = $state(false);
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
	defaultView = $state<ClaudeView>(lsGet(LS_VIEW) === 'terminal' ? 'terminal' : 'chat');
	claudeTerminals = $state<Record<string, ChatRef>>({});
	/** A chat ↔ terminal switch is stopping one process and starting the other. */
	switching = $state(false);
	/** Why the last action failed (switch, approve, open); shown on whichever screen is up. */
	notice = $state<string | null>(null);

	readonly agents = agentClient(() => ({ baseUrl: this.url, token: this.token }));

	machine = $derived(this.machines.list.find((m) => m.id === this.machineId) ?? null);
	activeTerminal = $derived(this.terminals.find((t) => t.id === this.activeTerminalId) ?? null);

	private readonly pairing: PairingScan;
	/** A URL that is already a complete origin (saved, or from a pairing code): never re-normalised. */
	private exactUrl: string | null;
	/** Bumped on every disconnect; a response for an older connection is dropped. */
	private generation = 0;

	constructor(scanner?: QrScanner) {
		this.pairing = new PairingScan(scanner);
		this.exactUrl = this.url || null;
	}

	/** Whether a machine was saved as active (auto-reconnect on launch). */
	get hasSavedServer(): boolean {
		return !!this.machines.active;
	}

	private authHeaders(): Record<string, string> {
		return { authorization: `Bearer ${this.token}` };
	}

	/** Connect to the form's server, leaving the current one; on success it is saved and active. */
	async connect(): Promise<void> {
		this.disconnect();
		const generation = this.generation;
		const stale = () => generation !== this.generation;
		this.connecting = true;
		try {
			// Only hand-typed input gets a scheme and the default port added: a scanned
			// `https://box.ts.net` must not become `https://box.ts.net:4317`.
			const base = this.url === this.exactUrl ? this.url : normalizeUrl(this.url);
			if (!base) throw new Error('enter a server address');
			this.token = this.token.trim();
			// Every Workbench server requires a token (see Settings → Server mode).
			if (!this.token) throw new Error('enter the server token');
			const res = await fetch(`${base}/health`);
			if (!res.ok) throw new Error(`health check returned ${res.status}`);
			// /health is unauthenticated, so check the token on a protected route.
			const authed = await fetch(`${base}/remote/terminals`, { headers: this.authHeaders() });
			if (authed.status === 401) throw new Error('invalid token');
			if (!authed.ok) throw new Error(`server returned ${authed.status}`);
			if (stale()) return;

			this.url = base;
			this.exactUrl = base;
			const machine = this.machines.save(base, this.token);

			const transport = createHttpTransport({ baseUrl: base, token: this.token });
			const next = new ControlPlaneStore(transport);
			await next.refresh();
			if (stale()) return next.dispose();
			this.machineId = machine.id;
			this.claudeTerminals = readLinks(machine.id);
			this.online = true;
			this.store = next;
			await Promise.all([this.refreshTerminals(), this.refreshChats()]);
		} catch (e) {
			if (!stale()) this.connectError = errorText(e);
		} finally {
			if (!stale()) this.connecting = false;
		}
	}

	/** Leave the current machine for a saved one. */
	async switchTo(id: string): Promise<void> {
		const machine = this.machines.list.find((m) => m.id === id);
		if (machine) await this.connectTo(machine.url, machine.token);
	}

	/** Connect to a complete origin (saved or scanned), never re-normalised. */
	private async connectTo(url: string, token: string): Promise<void> {
		this.url = url;
		this.exactUrl = url;
		this.token = token;
		await this.connect();
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
		const current = id === this.machineId;
		this.machines.remove(id);
		if (current) this.addMachine();
	}

	/** Scan the desktop's pairing QR code, then connect to (and save) that machine. */
	async scanAndConnect(): Promise<void> {
		if (this.pairing.scanning) return;
		this.connectError = null;
		try {
			const pairing = await this.pairing.scan();
			if (pairing) await this.connectTo(pairing.url, pairing.token);
		} catch (e) {
			this.connectError = errorText(e);
		}
	}

	get scanning(): boolean {
		return this.pairing.scanning;
	}

	/** Overlay Cancel / Android back. */
	cancelScan = (): Promise<void> => this.pairing.cancel();

	/** Close the connection: every screen of it goes, and late responses for it are dropped. */
	disconnect(): void {
		this.generation++;
		this.store?.dispose();
		this.store = null;
		this.machineId = null;
		this.connecting = false;
		this.connectError = null;
		this.claudeTerminals = {};
		this.terminals = [];
		this.chats = [];
		this.activeTerminalId = null;
		this.activeChat = null;
		this.notice = null;
	}

	setDefaultView(view: ClaudeView): void {
		this.defaultView = view;
		lsSet(LS_VIEW, view);
	}

	async refreshChats(): Promise<void> {
		if (!this.store) return;
		const generation = this.generation;
		try {
			const chats = await this.agents.list();
			if (generation === this.generation) this.chats = chats;
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
		const ref: ChatRef = { sessionId: crypto.randomUUID(), projectPath, worktreePath, name };
		if (this.defaultView === 'chat') this.openChat(ref);
		else await this.openClaudeTerminal(ref, false);
	};

	chatRef(chat: AgentSummary): ChatRef {
		return {
			sessionId: chat.sessionId,
			projectPath: chat.projectPath,
			worktreePath: chat.worktreePath ?? undefined,
			name: chat.title ?? baseName(chat.worktreePath ?? chat.projectPath),
			...(chat.claudeAccountId ? { claudeAccountId: chat.claudeAccountId } : {})
		};
	}

	openChat(ref: ChatRef): void {
		this.notice = null;
		this.activeTerminalId = null;
		this.activeChat = ref;
	}

	/** Arrow field — the chat view's Back. The session keeps running on the server. */
	closeChat = (): void => {
		this.activeChat = null;
		void this.refreshChats();
	};

	/** Answer an approval from the home screen, without opening the chat. */
	async answer(sessionId: string, requestId: string, decision: ApprovalDecision): Promise<void> {
		this.notice = null;
		try {
			await this.agents.send(sessionId, { t: 'approve', requestId, decision });
		} catch (e) {
			this.notice = `Couldn't answer Claude: ${errorText(e)}`;
		}
		await this.refreshChats();
	}

	/** End a chat session's `claude` process and leave its screen; the conversation stays on disk. */
	async endChat(sessionId: string): Promise<void> {
		await this.agents.stop(sessionId).catch(() => {});
		this.activeChat = null;
		await this.refreshChats();
	}

	/**
	 * Chat → terminal: stop the chat's process first (one writer per session
	 * file), then continue the conversation in a real `claude`.
	 */
	async showAsTerminal(ref: ChatRef, hasHistory: boolean): Promise<void> {
		this.switching = true;
		this.notice = null;
		try {
			await this.agents.stop(ref.sessionId);
		} catch (e) {
			this.notice = `Couldn't stop the chat: ${errorText(e)}`;
			this.switching = false;
			return;
		}
		this.activeChat = null;
		await this.openClaudeTerminal(ref, hasHistory);
		this.switching = false;
	}

	/** Terminal → chat: end the terminal's `claude`, then pick the conversation up in chat. */
	async showAsChat(terminalId: string): Promise<void> {
		const ref = this.claudeTerminals[terminalId];
		if (!ref) return;
		this.switching = true;
		this.notice = null;
		// Wait until the terminal's process group is gone: two `claude`s on one
		// session would both write its transcript.
		const stopped = await this.deleteTerminal(terminalId, true);
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
		const generation = this.generation;
		const current = () => generation === this.generation;
		try {
			const res = await fetch(`${this.url}/remote/terminals`, { headers: this.authHeaders() });
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
		const generation = this.generation;
		try {
			const res = await fetch(`${this.url}/remote/terminals`, {
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
			if (generation !== this.generation) return null;
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
			ensureVisible();
			return meta.id;
		} catch (e) {
			this.notice = `Couldn't open a terminal: ${errorText(e)}`;
			return null;
		}
	};

	/** `wait` returns only once its processes are gone. */
	private async deleteTerminal(id: string, wait = false): Promise<boolean> {
		try {
			const res = await fetch(
				`${this.url}/remote/terminals/${encodeURIComponent(id)}${wait ? '?wait=true' : ''}`,
				{ method: 'DELETE', headers: this.authHeaders() }
			);
			if (!res.ok) return false;
		} catch {
			return false;
		}
		if (this.activeTerminalId === id) this.activeTerminalId = null;
		return true;
	}

	async killTerminal(id: string): Promise<void> {
		await this.deleteTerminal(id);
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
