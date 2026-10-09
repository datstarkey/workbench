import { agentClient, agentName, type AgentApi } from '@workbench/chat-ui';
import { ControlPlaneStore } from '@workbench/control-plane-ui';
import { createHttpTransport } from '@workbench/transport';
import type {
	AgentKind,
	ApprovalDecision,
	ClaudeAccount,
	WorkbenchSettings
} from '@workbench/types';
import { defaultAccountName, projectClaudeAccount } from '@workbench/types';
import { openUrl } from '@tauri-apps/plugin-opener';
import { HostUpdate } from './host-update.svelte.ts';
import { LegacyRemote } from './legacy-remote.svelte.ts';
import { hostOf, machineKey, normalizeUrl, SavedMachines } from './machines.svelte.ts';
import { findPane, paneEntries, paneTitle, type PaneEntry } from './panes.ts';
import { PairingScan, type QrScanner } from './qr-scan.svelte.ts';
import { WorkspaceRemote, type PaneRemote } from './remote.svelte.ts';
import { verifyServer } from './server-check.ts';
import { lsGet, lsSet } from './storage.ts';
import { ProjectPrefs } from './project-prefs.svelte.ts';
import { Drafts } from './drafts.svelte';
import { SessionNotifications, type NotificationSession } from './session-notifications.svelte';
import { ProjectReview, type ReviewFolder } from './project-review.svelte';
import type { ChatRef, ClaudeView } from './types.ts';
import type {
	OpenEventSource,
	PaneKind,
	WorkspaceCommand,
	WorkspacePane
} from './workspace-stream.ts';

const LS_VIEW = 'wb.claudeView';
/** While a host restarts into its update, check when it is back. */
const UPDATE_CHECK_MS = 4000;

function errorText(e: unknown): string {
	return e instanceof Error ? e.message : String(e);
}

/** Uses the system URL handler without the opener plugin's inAppBrowser mode. */
export function openExternal(url: string): void {
	openUrl(url).catch((e) => console.warn('[mobile] open url', url, e));
}

export type Folder = Pick<ReviewFolder, 'projectPath' | 'worktreePath'>;

/**
 * The phone is a remote for the connected machine's workspaces: it renders
 * them from the host and sends commands; every process lives on the host. Kept
 * out of the components so it can be unit-tested.
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
	/** The connected machine's workspaces; null while disconnected. */
	remote = $state.raw<PaneRemote | null>(null);
	drafts = new Drafts('disconnected');
	projectPrefs = $state.raw(new ProjectPrefs('disconnected'));
	accounts = $state<Pick<ClaudeAccount, 'id' | 'name'>[]>([]);
	/** What the machine calls its default `~/.claude` account. */
	defaultAccountName = $state(defaultAccountName(null));
	accountId = $state<string | undefined>(undefined);
	/** The connected host's version and update; null while disconnected. */
	hostUpdate = $state.raw<HostUpdate | null>(null);
	connecting = $state(false);
	/** The saved machine a connect is in flight to (null for one not saved yet). */
	connectingTo = $state<string | null>(null);
	connectError = $state<string | null>(null);
	/** The connected machine's id (null while disconnected). */
	machineId = $state<string | null>(null);
	defaultView = $state<ClaudeView>(lsGet(LS_VIEW) === 'terminal' ? 'terminal' : 'chat');
	/** Why the last action failed; shown on whichever screen is up. */
	notice = $state<string | null>(null);
	/** The pane whose screen is open. It closes when the pane leaves the host's model. */
	openPaneId = $state<string | null>(null);
	/** Chat or Terminal per Claude pane: this phone's presentation only. */
	views = $state<Record<string, ClaudeView>>({});
	/** Bumped to remount the chat screen (it moved to another Claude account). */
	chatScreenKey = $state(0);
	private chatAccounts: Record<string, string | undefined> = {};
	private controlPlane: ReturnType<typeof createHttpTransport> | null = null;

	readonly agents = agentClient(() => ({
		baseUrl: this.connection?.url ?? '',
		token: this.connection?.token ?? ''
	}));
	/** Chat screens only ever attach: starting a process is a command. */
	readonly attachApi: AgentApi = {
		...this.agents,
		start: (body) => this.agents.start({ ...body, attachOnly: true })
	};

	machine = $derived(this.machines.list.find((m) => m.id === this.machineId) ?? null);
	panes = $derived(paneEntries(this.remote?.workspaces ?? []));
	activePane = $derived(this.panes.find((e) => e.pane.id === this.openPaneId) ?? null);
	online = $derived(this.remote?.online ?? true);

	private readonly pairing: PairingScan;
	private readonly openEventSource?: OpenEventSource;
	/** A URL that is already a complete origin (saved, or from a pairing code): never re-normalised. */
	private exactUrl: string | null;
	/** Bumped whenever the connection goes; a response for an older connection is dropped. */
	private generation = 0;
	/** Bumped on every connect attempt; a newer attempt, a disconnect or forgetting its machine supersedes it. */
	private attempt = 0;
	/** Whether the app is in front: the host's model is followed only then. */
	private visible = !document.hidden;

	constructor(scanner?: QrScanner, openEventSource?: OpenEventSource) {
		this.pairing = new PairingScan(scanner);
		this.openEventSource = openEventSource;
		this.exactUrl = this.url || null;
	}

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
			this.defaultAccountName = defaultAccountName(settings);
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

	/** Whether a machine was saved as active (auto-reconnect on launch). */
	get hasSavedServer(): boolean {
		return !!this.machines.active;
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
			const health = await verifyServer(base, token);
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
			this.store = next;
			this.hostUpdate = new HostUpdate(controlPlane);
			void this.hostUpdate.check();
			// OLD-HOST FALLBACK: a host without `workspaceApi` (Phase 5 deletes this branch).
			const remote: PaneRemote = health.workspaceApi
				? new WorkspaceRemote(this.connection, this.openEventSource)
				: new LegacyRemote(this.connection, this.openEventSource);
			this.remote = remote;
			remote.follow(this.visible);
			await Promise.all([remote instanceof LegacyRemote && remote.refresh(), this.loadAccounts()]);
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
		this.remote?.dispose();
		this.remote = null;
		this.store = null;
		this.connection = null;
		this.controlPlane = null;
		this.hostUpdate = null;
		this.accounts = [];
		this.defaultAccountName = defaultAccountName(null);
		this.accountId = undefined;
		this.machineId = null;
		this.projectPrefs = new ProjectPrefs('disconnected');
		this.openPaneId = null;
		this.views = {};
		this.chatAccounts = {};
		this.notice = null;
	}

	setDefaultView(view: ClaudeView): void {
		this.defaultView = view;
		lsSet(LS_VIEW, view);
	}

	/**
	 * Follow the host while the app is in front, and catch up as soon as it
	 * comes back from the lock screen. Returns a stop function.
	 */
	watch(): () => void {
		this.remote?.follow(this.visible);
		const timer = setInterval(() => {
			// The host is restarting into its update: its first answer ends "Updating host…".
			if (!document.hidden && this.hostUpdate?.updating) void this.hostUpdate.check();
		}, UPDATE_CHECK_MS);
		const wake = () => {
			this.visible = !document.hidden;
			this.remote?.follow(this.visible);
			if (this.visible) {
				void this.store?.refresh();
				void this.hostUpdate?.check();
			}
		};
		document.addEventListener('visibilitychange', wake);
		return () => {
			clearInterval(timer);
			document.removeEventListener('visibilitychange', wake);
			this.remote?.follow(false);
		};
	}

	/** Send a command; a refusal or failure shows as a notice and resolves null. */
	private async send(cmd: WorkspaceCommand, failure: string) {
		const live = this.live();
		const remote = this.remote;
		if (!remote) return null;
		this.notice = null;
		try {
			const result = await remote.command(cmd);
			return live() ? result : null;
		} catch (e) {
			if (live()) this.notice = `${failure}: ${errorText(e)}`;
			return null;
		}
	}

	/** A new session in a folder; it opens here as a tab on the host, too. */
	start = (kind: PaneKind, folder: Folder, view?: ClaudeView) =>
		this.newSession(kind, folder, {}, view);

	/** Continue a past conversation (the history sheet). A pane already running it is reused. */
	resume(kind: AgentKind, folder: Folder, sessionId: string, accountId?: string): Promise<void> {
		return this.newSession(kind, folder, { resume: sessionId, accountId });
	}

	private async newSession(
		kind: PaneKind,
		{ projectPath, worktreePath }: Folder,
		opts: { resume?: string; accountId?: string },
		view?: ClaudeView
	): Promise<void> {
		const project = this.store?.projects.find((p) => p.path === projectPath);
		const branch = worktreePath
			? this.store?.worktrees[projectPath]?.find((w) => w.path === worktreePath)?.branch
			: undefined;
		const accountId =
			kind === 'claude'
				? (opts.accountId ?? projectClaudeAccount(project, this.accounts, this.accountId))
				: undefined;
		const result = await this.send(
			{
				type: 'newSession',
				projectPath,
				...(worktreePath ? { worktreePath } : {}),
				...(project ? { projectName: project.name } : {}),
				...(branch ? { branch } : {}),
				kind,
				...(opts.resume ? { resume: opts.resume } : {}),
				...(accountId ? { accountId } : {}),
				...(kind === 'codex' ? { codexMode: 'appServer' as const } : {})
			},
			kind === 'shell' ? "Couldn't open a terminal" : `Couldn't start ${agentName(kind)}`
		);
		if (result?.paneId) this.openPane(result.paneId, view);
	}

	/** Show a pane's screen; `view` picks Chat or Terminal for a Claude pane. */
	openPane(paneId: string, view?: ClaudeView): void {
		this.notice = null;
		const known = this.panes.map((e) => e.pane.id);
		const views = Object.fromEntries(
			Object.entries(this.views).filter(([id]) => known.includes(id))
		);
		this.views = view ? { ...views, [paneId]: view } : views;
		this.openPaneId = paneId;
	}

	/** Back: navigation only. The session keeps running on the host. */
	closeScreen = (): void => {
		this.openPaneId = null;
	};

	/** What the phone shows for a pane: Claude per the phone's pick, Codex by its process. */
	paneView(pane: WorkspacePane): ClaudeView {
		if (pane.kind === 'shell') return 'terminal';
		if (pane.kind === 'codex') return pane.codexMode === 'appServer' ? 'chat' : 'terminal';
		return this.views[pane.id] ?? this.defaultView;
	}

	setView(paneId: string, view: ClaudeView): void {
		this.notice = null;
		this.views = { ...this.views, [paneId]: view };
	}

	/** End: the pane goes on every device, like the desktop's ×. */
	async endPane(paneId: string): Promise<void> {
		const result = await this.send({ type: 'closePane', paneId }, "Couldn't end the session");
		if (result && this.openPaneId === paneId) this.openPaneId = null;
	}

	restart = (tabId: string) =>
		this.send({ type: 'restart', tabId }, "Couldn't restart the session");

	trustFolder = (paneId: string) =>
		this.send({ type: 'trustFolder', paneId }, "Couldn't trust the folder");

	chatRef(entry: PaneEntry): ChatRef {
		const { workspace, pane } = entry;
		const accountId = pane.id in this.chatAccounts ? this.chatAccounts[pane.id] : pane.accountId;
		return {
			sessionId: pane.sessionId ?? '',
			...(pane.kind === 'codex' ? { agent: 'codex' as const } : {}),
			projectPath: workspace.projectPath,
			...(workspace.worktreePath ? { worktreePath: workspace.worktreePath } : {}),
			name: paneTitle(entry),
			...(accountId ? { claudeAccountId: accountId } : {})
		};
	}

	/** The open chat moved to another Claude account: remount it for that login's usage. */
	updateChatAccount(paneId: string, accountId: string | undefined): void {
		this.chatAccounts[paneId] = accountId;
		this.chatScreenKey++;
	}

	/** A notification opens the pane that runs its session; it never starts one. */
	async openNotification(chat: NotificationSession): Promise<void> {
		const live = this.live();
		const remote = this.remote;
		if (!remote) return;
		const find = () =>
			findPane(this.panes, {
				sessionId: chat.sessionId,
				terminalId: chat.terminalOnly ? chat.terminalId : null
			});
		// A just-switched machine may not have shown its workspaces yet.
		if (!find()) {
			await remote.refresh();
			if (!live()) return;
		}
		const found = find();
		if (!found) {
			this.openPaneId = null;
			this.notice = chat.terminalOnly
				? 'This Codex terminal is available on the desktop.'
				: 'That session is no longer running.';
			return;
		}
		this.openPane(found.pane.id, chat.terminalOnly ? 'terminal' : 'chat');
	}

	/** Answer an approval from the home screen, without opening the chat. */
	async answer(sessionId: string, requestId: string, decision: ApprovalDecision): Promise<void> {
		const live = this.live();
		const kind = findPane(this.panes, { sessionId })?.pane.kind;
		this.notice = null;
		try {
			await this.agents.send(sessionId, { t: 'approve', requestId, decision });
		} catch (e) {
			if (live())
				this.notice = `Couldn't answer ${agentName(kind === 'codex' ? 'codex' : 'claude')}: ${errorText(e)}`;
		}
	}

	/** Reload the projects, the workspaces and the host's update. */
	refreshAll(): void {
		void this.store?.refresh();
		void this.remote?.refresh();
		void this.hostUpdate?.check();
	}
}
