import { ControlPlaneStore } from '@workbench/control-plane-ui';
import { createHttpTransport } from '@workbench/transport';

export type TerminalMeta = {
	id: string;
	name?: string;
	cwd: string;
	createdAt: number;
	alive: boolean;
};

const LS_URL = 'wb.serverUrl';
const LS_TOKEN = 'wb.token';
const DEFAULT_PORT = '4317';

// localStorage can throw in some webview contexts — never let it crash mount.
function lsGet(key: string): string | null {
	try {
		return localStorage.getItem(key);
	} catch {
		return null;
	}
}
function lsSet(key: string, value: string) {
	try {
		localStorage.setItem(key, value);
	} catch {
		/* ignore */
	}
}

// Accept a bare Tailscale IP / host: add http:// and the default port so you can
// just paste the IP.
export function normalizeUrl(raw: string): string {
	let s = raw.trim();
	if (!s) return s;
	if (!/^https?:\/\//.test(s)) s = `http://${s}`;
	try {
		const u = new URL(s);
		if (!u.port) u.port = DEFAULT_PORT;
		return u.toString().replace(/\/$/, '');
	} catch {
		return s.replace(/\/$/, '');
	}
}

/**
 * Phone-side connection + terminal state for the mobile app. Owns the
 * control-plane store (over HTTP) plus the persistent-terminal list and the
 * active terminal. Kept out of the component so it can be unit-tested.
 */
export class MobileClient {
	url = $state(lsGet(LS_URL) ?? '');
	token = $state(lsGet(LS_TOKEN) ?? '');
	store = $state<ControlPlaneStore | null>(null);
	connecting = $state(false);
	connectError = $state<string | null>(null);
	terminals = $state<TerminalMeta[]>([]);
	activeTerminalId = $state<string | null>(null);

	serverLabel = $derived(this.url.replace(/^https?:\/\//, ''));
	activeTerminal = $derived(this.terminals.find((t) => t.id === this.activeTerminalId) ?? null);

	/** Whether a server address was previously saved (auto-reconnect on launch). */
	get hasSavedServer(): boolean {
		return !!lsGet(LS_URL);
	}

	private authHeaders(): Record<string, string> {
		return this.token ? { authorization: `Bearer ${this.token}` } : {};
	}

	async connect(): Promise<void> {
		this.connecting = true;
		this.connectError = null;
		try {
			const base = normalizeUrl(this.url);
			if (!base) throw new Error('enter a server address');
			const res = await fetch(`${base}/health`, { headers: this.authHeaders() });
			if (!res.ok) throw new Error(`health check returned ${res.status}`);

			this.url = base;
			lsSet(LS_URL, base);
			lsSet(LS_TOKEN, this.token);

			const transport = createHttpTransport({ baseUrl: base, token: this.token || undefined });
			const next = new ControlPlaneStore(transport);
			await next.refresh();
			this.store = next;
			await this.refreshTerminals();
		} catch (e) {
			this.connectError = e instanceof Error ? e.message : String(e);
		} finally {
			this.connecting = false;
		}
	}

	disconnect(): void {
		this.store?.dispose();
		this.store = null;
		this.terminals = [];
		this.activeTerminalId = null;
	}

	async refreshTerminals(): Promise<void> {
		if (!this.store) return;
		try {
			const res = await fetch(`${this.url}/remote/terminals`, { headers: this.authHeaders() });
			if (res.ok) {
				const data = await res.json();
				// Guard the {#each terminals} render: a non-array body would throw.
				this.terminals = Array.isArray(data) ? data : [];
			}
		} catch {
			/* ignore */
		}
	}

	/** Arrow field so it can be passed straight to ControlPlaneSidebar's
	 *  onOpenTerminal callback without losing `this`. */
	createTerminal = async (
		projectPath: string,
		worktreePath: string | undefined,
		name: string,
		command?: string
	): Promise<void> => {
		if (!this.store) return;
		try {
			const res = await fetch(`${this.url}/remote/terminals`, {
				method: 'POST',
				headers: { 'content-type': 'application/json', ...this.authHeaders() },
				body: JSON.stringify({ projectPath, worktreePath, name, command, cols: 80, rows: 24 })
			});
			if (!res.ok) throw new Error(`create terminal failed (${res.status})`);
			const meta: TerminalMeta = await res.json();
			// Open the new terminal immediately, then reconcile with the server list —
			// otherwise a list that hasn't yet surfaced the new id leaves activeTerminal
			// null and the view never opens.
			if (!this.terminals.some((t) => t.id === meta.id)) {
				this.terminals = [...this.terminals, meta];
			}
			this.activeTerminalId = meta.id;
			await this.refreshTerminals();
		} catch (e) {
			this.connectError = e instanceof Error ? e.message : String(e);
		}
	};

	async killTerminal(id: string): Promise<void> {
		try {
			await fetch(`${this.url}/remote/terminals/${id}`, {
				method: 'DELETE',
				headers: this.authHeaders()
			});
		} catch {
			/* ignore */
		}
		if (this.activeTerminalId === id) this.activeTerminalId = null;
		await this.refreshTerminals();
	}

	selectTerminal(id: string): void {
		this.activeTerminalId = id;
	}

	/** Arrow field — passed as the Terminal view's onClose callback. */
	closeTerminal = (): void => {
		this.activeTerminalId = null;
		void this.refreshTerminals();
	};

	/** Header "Refresh": reload both the control plane and the terminal list. */
	refreshAll(): void {
		void this.store?.refresh();
		void this.refreshTerminals();
	}
}
