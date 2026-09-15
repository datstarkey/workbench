import { ControlPlaneStore } from '@workbench/control-plane-ui';
import { createHttpTransport, parsePairingUri, type PairingInfo } from '@workbench/transport';
import * as barcodeScanner from '@tauri-apps/plugin-barcode-scanner';
import { onBackButtonPress } from '@tauri-apps/api/app';
import type { PluginListener } from '@tauri-apps/api/core';

/** The plugin surface pairing uses (injectable for tests). */
export type QrScanner = Pick<
	typeof barcodeScanner,
	'checkPermissions' | 'requestPermissions' | 'scan' | 'cancel'
> & { onBackButtonPress: typeof onBackButtonPress };

const defaultScanner: QrScanner = { ...barcodeScanner, onBackButtonPress };

export const CAMERA_DENIED =
	'Camera permission denied. Allow it in system settings, or enter the server details below.';
export const NOT_A_PAIRING_CODE = 'Not a Workbench pairing code';

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

	scanning = $state(false);

	serverLabel = $derived(this.url.replace(/^https?:\/\//, ''));
	activeTerminal = $derived(this.terminals.find((t) => t.id === this.activeTerminalId) ?? null);

	private readonly scanner: QrScanner;
	/** Bumped on every scan start and cancel; a scan whose number is stale is ignored. */
	private scanGeneration = 0;
	private backButton: PluginListener | null = null;
	/** A URL that is already a complete origin (saved, or from a pairing code): never re-normalised. */
	private exactUrl: string | null;

	constructor(scanner: QrScanner = defaultScanner) {
		this.scanner = scanner;
		this.exactUrl = this.url || null;
	}

	/** Whether a server address and token were previously saved (auto-reconnect on launch). */
	get hasSavedServer(): boolean {
		return !!lsGet(LS_URL) && !!lsGet(LS_TOKEN);
	}

	private authHeaders(): Record<string, string> {
		return { authorization: `Bearer ${this.token}` };
	}

	async connect(): Promise<void> {
		this.connecting = true;
		this.connectError = null;
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

			this.url = base;
			this.exactUrl = base;
			lsSet(LS_URL, base);
			lsSet(LS_TOKEN, this.token);

			const transport = createHttpTransport({ baseUrl: base, token: this.token });
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

	/**
	 * Scan the desktop's pairing QR code (Settings → Server mode → Pair phone),
	 * fill in the server details and connect.
	 *
	 * The scan is windowed: the camera renders behind the (transparent) webview so
	 * our overlay can offer Cancel. Cancel and the Android back button end the scan
	 * here rather than waiting on the plugin, whose promise never settles once
	 * cancelled (nor on a device without a camera).
	 */
	async scanAndConnect(): Promise<void> {
		if (this.scanning) return;
		this.connectError = null;
		this.scanning = true;
		const generation = ++this.scanGeneration;
		const stale = () => generation !== this.scanGeneration;
		let pairing: PairingInfo | null;
		try {
			let permission = await this.scanner.checkPermissions();
			if (permission !== 'granted') permission = await this.scanner.requestPermissions();
			if (stale()) return;
			if (permission !== 'granted') {
				this.connectError = CAMERA_DENIED;
				return;
			}
			const listener = await this.scanner.onBackButtonPress(() => void this.cancelScan());
			if (stale()) {
				void listener.unregister();
				return;
			}
			this.backButton = listener;
			const { content } = await this.scanner.scan({
				windowed: true,
				formats: [barcodeScanner.Format.QRCode]
			});
			if (stale()) return;
			pairing = parsePairingUri(content);
			if (!pairing) {
				this.connectError = NOT_A_PAIRING_CODE;
				return;
			}
		} catch (e) {
			if (stale()) return;
			const message = e instanceof Error ? e.message : String(e);
			if (!/cancel/i.test(message)) this.connectError = message;
			return;
		} finally {
			if (!stale()) this.endScan();
		}
		this.url = pairing.url;
		this.exactUrl = pairing.url;
		this.token = pairing.token;
		await this.connect();
	}

	/** Overlay Cancel / Android back: end the scan now; a late result is ignored. */
	cancelScan = async (): Promise<void> => {
		if (!this.scanning) return;
		this.scanGeneration++;
		this.endScan();
		try {
			await this.scanner.cancel();
		} catch {
			/* nothing was scanning on the native side */
		}
	};

	private endScan(): void {
		this.scanning = false;
		void this.backButton?.unregister();
		this.backButton = null;
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
			this.activeTerminalId = meta.id;
			await this.refreshTerminals();
			ensureVisible();
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
