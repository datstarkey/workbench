/**
 * WS-backed terminal connection for xterm panes.
 *
 * Each xterm pane owns one `TerminalConnection`. It talks to the embedded
 * server's TerminalManager via the same wire protocol as the mobile client:
 *   client → server  text JSON  {"t":"i","d":…} input
 *                               {"t":"r","c":…,"r":…} resize
 *   server → client  binary     raw PTY bytes
 *                    text JSON  {"t":"takeover"} or {"t":"exit","code":N|null}
 *
 * Boot sequence
 * ─────────────
 * 1. `connect()` calls `terminalServerStatus()` to get the always-on loopback
 *    address/token (NOT the opt-in LAN server).
 * 2. If a persisted server-terminal id is passed and still alive on the server,
 *    re-attach to it; otherwise POST /remote/terminals to create a fresh PTY.
 * 3. Open WebSocket ws://<addr>/remote/terminals/<id>/ws[?token=…].
 * 4. On WS `open` send an initial resize so the PTY starts at the correct size.
 *
 * Single-attach lease
 * ───────────────────
 * The server enforces single-attach: if a second client attaches, the first
 * receives a {"t":"takeover"} frame then its socket is closed. `onExit` is
 * fired with `{ reason: 'taken_over' }` in that case, and the pane stays
 * detached until the user calls `takeControl()` — never automatically, or two
 * devices would kick each other back and forth.
 *
 * PTY persistence
 * ───────────────
 * The PTY lives in the server process. Closing the WS (navigate away, webview
 * reload) just detaches — the shell keeps running and can be resumed by passing
 * the same server-terminal id back to `connect()`. Use `deleteServerTerminal()`
 * to actually kill the PTY when the user intentionally closes the pane.
 */

import { terminalServerStatus } from '$lib/server-mode';
import { parseTerminalControlFrame, terminalWsUrl } from '@workbench/transport';

/** Payload delivered to the `onData` callback. */
export type TerminalDataPayload = Uint8Array;

/** Reason a terminal session ended from the client's perspective. */
export type ExitReason = 'ended' | 'taken_over';

export interface TerminalExitInfo {
	reason: ExitReason;
	/** Exit code from the shell; present only when `reason === 'ended'`. */
	code?: number;
}

export interface ConnectOptions {
	/** Project or worktree path the PTY should `cd` to. */
	projectPath: string;
	/** Override with the worktree path when applicable. */
	worktreePath?: string;
	/** Display name shown in terminal lists. */
	name?: string;
	/** Optional startup command to run after the shell starts (e.g. `claude`). */
	command?: string;
	/** Initial terminal width in columns. */
	cols: number;
	/** Initial terminal height in rows. */
	rows: number;
	/** Stable pane id — forwarded as WORKBENCH_PANE_ID for hook correlation. */
	paneId?: string;
	/** Project-configured shell — forwarded so the server launches the right one. */
	shell?: string;
	/** Hook-bridge socket address — forwarded as WORKBENCH_HOOK_SOCKET. */
	hookSocket?: string;
}

/** Server-side terminal metadata returned by GET/POST /remote/terminals. */
export interface TerminalMeta {
	id: string;
	name?: string;
	cwd: string;
	createdAt: number;
	alive: boolean;
}

/** Resolved loopback server coordinates. */
interface ServerInfo {
	baseUrl: string;
	token?: string;
}

/**
 * Cached loopback server coordinates. The embedded server boots once at startup
 * and keeps its ephemeral port for the process lifetime, so every pane resolves
 * the same address — memoize it instead of doing one IPC round-trip per pane.
 * Cleared on failure so a probe before the server is up stays retryable.
 */
let serverInfoCache: Promise<ServerInfo> | null = null;

function resolveServer(): Promise<ServerInfo> {
	if (!serverInfoCache) {
		serverInfoCache = (async () => {
			const status = await terminalServerStatus();
			if (!status.running || !status.address) {
				throw new Error('embedded server is not running');
			}
			return { baseUrl: `http://${status.address}`, token: status.token ?? undefined };
		})();
		serverInfoCache.catch(() => {
			serverInfoCache = null;
		});
	}
	return serverInfoCache;
}

function authHeaders(token?: string): Record<string, string> {
	return token ? { authorization: `Bearer ${token}` } : {};
}

/**
 * Server terminal ids this webview created, attached to or killed. Adoption
 * skips them: the loopback list is shared with other devices, and a pane's own
 * PTY is visible there before its id reaches the workspace store (or, after a
 * kill, until the DELETE lands).
 */
const claimedIds = new Set<string>();
/** Creates whose id isn't known yet — the server may already list it. */
let pendingCreates = 0;

export function isClaimedLocally(id: string): boolean {
	return claimedIds.has(id);
}

/** Test-only: drop the memoized server-info cache and claims so tests stay isolated. */
export function __resetServerInfoCache(): void {
	serverInfoCache = null;
	claimedIds.clear();
	pendingCreates = 0;
}

/**
 * List the loopback server's terminals (including ones opened from other
 * devices). Null when unavailable — including while a local create is in
 * flight, whose not-yet-claimed id would look like a foreign terminal.
 */
export async function listServerTerminals(): Promise<TerminalMeta[] | null> {
	try {
		const { baseUrl, token } = await resolveServer();
		const resp = await fetch(`${baseUrl}/remote/terminals`, { headers: authHeaders(token) });
		if (!resp.ok) return null;
		const list: unknown = await resp.json();
		if (pendingCreates > 0 || !Array.isArray(list)) return null;
		return list as TerminalMeta[];
	} catch {
		return null;
	}
}

/**
 * Manages a single WebSocket connection to an embedded-server terminal session.
 *
 * Lifecycle:
 *   1. Construct with `onData` / `onExit` / `onReset` callbacks.
 *   2. Call `connect(opts, existingId?)` — async, resolves once the WS is open
 *      and the initial resize frame has been sent.
 *   3. Use `write()` / `resize()` to drive the PTY.
 *   4. Call `dispose()` to detach (close the WS — PTY keeps running).
 */
export class TerminalConnection {
	/** Server-assigned terminal id, available after `connect()` resolves. */
	terminalId: string | null = null;

	private ws: WebSocket | null = null;
	/** Options from the last connect, reused by `takeControl()`. */
	private lastOpts: ConnectOptions | null = null;
	private readonly onData: (data: TerminalDataPayload) => void;
	private readonly onExit: (info: TerminalExitInfo) => void;
	private readonly onReset?: () => void;

	/** True once we intentionally detached — suppresses the onclose exit. */
	private disposed = false;
	/**
	 * True once an exit/takeover has been surfaced to `onExit`. The server sends a
	 * control frame THEN closes the socket, so without this the onclose handler
	 * would fire a second, contradictory `onExit`.
	 */
	private exitDelivered = false;

	/**
	 * @param onData  Called with raw PTY output bytes as they arrive.
	 * @param onExit  Called exactly once when the session ends or is taken over.
	 * @param onReset Called before scrollback replay (used to clear xterm's
	 *                viewport so the replay doesn't double-print old output).
	 */
	constructor(
		onData: (data: TerminalDataPayload) => void,
		onExit: (info: TerminalExitInfo) => void,
		onReset?: () => void
	) {
		this.onData = onData;
		this.onExit = onExit;
		this.onReset = onReset;
	}

	/** Fire `onExit` at most once across the control-frame and onclose paths. */
	private deliverExit(info: TerminalExitInfo): void {
		if (this.exitDelivered) return;
		this.exitDelivered = true;
		this.onExit(info);
	}

	/**
	 * Re-attach to a persisted PTY if it is still alive, else create a fresh one.
	 *
	 * @param opts        Spec for a freshly-created PTY.
	 * @param existingId  Persisted server-terminal id to re-attach to (webview
	 *                    reload survival). Ignored if the PTY no longer exists.
	 *
	 * Resolves once the socket is open and the initial resize frame is sent.
	 */
	async connect(opts: ConnectOptions, existingId?: string): Promise<void> {
		this.lastOpts = opts;
		const { baseUrl, token } = await resolveServer();
		if (this.disposed) return;

		// Re-attach to the surviving PTY (no POST, scrollback replayed) when the
		// persisted id is still alive; otherwise create a fresh one.
		const reattach = existingId ? await this.isAlive(baseUrl, token, existingId) : false;
		if (this.disposed) return;
		const id = reattach ? existingId! : await this.createTerminal(baseUrl, token, opts);
		claimedIds.add(id);
		this.terminalId = id;

		// dispose() may have landed while we awaited create/isAlive — before this.ws
		// existed, so its close() was a no-op. Kill the freshly-created PTY (a
		// reattach leaves the existing one alone) and never open a socket.
		if (this.disposed) {
			if (!reattach) {
				void deleteServerTerminal(id);
				this.terminalId = null;
			}
			return;
		}

		await this.openSocket(terminalWsUrl(baseUrl, id, token), opts);
	}

	/**
	 * Remember an existing server terminal without attaching, so a device that
	 * currently holds it isn't kicked. The pane attaches on `takeControl()`.
	 */
	connectDetached(opts: ConnectOptions, existingId: string): void {
		this.lastOpts = opts;
		this.terminalId = existingId;
		claimedIds.add(existingId);
	}

	/**
	 * Re-attach after a takeover (or a detached start), kicking whichever device
	 * holds the terminal now. Falls back to a fresh PTY if it has since died, so
	 * callers should re-read `terminalId` afterwards.
	 */
	async takeControl(cols: number, rows: number): Promise<void> {
		if (!this.lastOpts) throw new Error('terminal was never connected');
		this.detachSocket();
		this.exitDelivered = false;
		await this.connect({ ...this.lastOpts, cols, rows }, this.terminalId ?? undefined);
	}

	/** Whether a server terminal with `id` still exists and is alive. */
	private async isAlive(baseUrl: string, token: string | undefined, id: string): Promise<boolean> {
		try {
			const resp = await fetch(`${baseUrl}/remote/terminals`, { headers: authHeaders(token) });
			if (!resp.ok) return false;
			const list = (await resp.json()) as TerminalMeta[];
			return Array.isArray(list) && list.some((t) => t.id === id && t.alive);
		} catch {
			return false;
		}
	}

	/** Create a server-side PTY and return its id. */
	private async createTerminal(
		baseUrl: string,
		token: string | undefined,
		opts: ConnectOptions
	): Promise<string> {
		pendingCreates += 1;
		try {
			return await this.postTerminal(baseUrl, token, opts);
		} finally {
			pendingCreates -= 1;
		}
	}

	private async postTerminal(
		baseUrl: string,
		token: string | undefined,
		opts: ConnectOptions
	): Promise<string> {
		const resp = await fetch(`${baseUrl}/remote/terminals`, {
			method: 'POST',
			headers: { 'content-type': 'application/json', ...authHeaders(token) },
			body: JSON.stringify({
				projectPath: opts.projectPath,
				worktreePath: opts.worktreePath ?? null,
				name: opts.name ?? null,
				command: opts.command ?? null,
				cols: opts.cols,
				rows: opts.rows,
				paneId: opts.paneId ?? null,
				shell: opts.shell || null,
				hookSocket: opts.hookSocket ?? null
			})
		});
		if (!resp.ok) {
			throw new Error(`POST /remote/terminals failed: ${resp.status}`);
		}
		const meta: TerminalMeta = await resp.json();
		claimedIds.add(meta.id);
		return meta.id;
	}

	/** Open the attach WebSocket and wire up the message/close handlers. */
	private openSocket(wsUrl: string, opts: ConnectOptions): Promise<void> {
		// Disposed between connect()'s guard and here → don't open a socket.
		if (this.disposed) return Promise.resolve();

		// The first binary frame is scrollback replay — reset xterm before it so
		// re-attached history isn't duplicated into the still-mounted terminal.
		let firstFrame = true;

		const ws = new WebSocket(wsUrl);
		ws.binaryType = 'arraybuffer';
		this.ws = ws;

		return new Promise<void>((resolve, reject) => {
			ws.onopen = () => {
				ws.send(JSON.stringify({ t: 'r', c: opts.cols, r: opts.rows }));
				resolve();
			};

			ws.onerror = () => {
				// Strip ?token= so the credential never lands in UI text or error reports.
				reject(new Error(`WebSocket error connecting to ${wsUrl.split('?')[0]}`));
			};

			ws.onmessage = (event: MessageEvent) => {
				if (event.data instanceof ArrayBuffer) {
					if (firstFrame) {
						firstFrame = false;
						this.onReset?.();
					}
					this.onData(new Uint8Array(event.data));
				} else if (typeof event.data === 'string') {
					const frame = parseTerminalControlFrame(event.data);
					if (frame?.t === 'takeover') {
						this.deliverExit({ reason: 'taken_over' });
					} else if (frame?.t === 'exit') {
						this.deliverExit({ reason: 'ended', code: frame.code ?? undefined });
					} else if (frame?.t === 'revoked') {
						this.deliverExit({ reason: 'ended' });
					}
				}
			};

			ws.onclose = () => {
				// A clean exit/takeover already delivered its control frame; an
				// intentional dispose() must stay silent. Only surface onExit for an
				// unexpected drop (network, server gone) with no prior control frame.
				if (this.disposed) return;
				this.deliverExit({ reason: 'ended' });
			};
		});
	}

	/** Send PTY input. No-op if the socket is not OPEN. */
	write(data: string): void {
		if (this.ws?.readyState === WebSocket.OPEN) {
			this.ws.send(JSON.stringify({ t: 'i', d: data }));
		}
	}

	/** Send a terminal resize. No-op if the socket is not OPEN. */
	resize(cols: number, rows: number): void {
		if (this.ws?.readyState === WebSocket.OPEN) {
			this.ws.send(JSON.stringify({ t: 'r', c: cols, r: rows }));
		}
	}

	/**
	 * Detach (close the WebSocket). The PTY keeps running on the server so it can
	 * be resumed. Does NOT fire `onExit` — this is an intentional teardown.
	 */
	dispose(): void {
		this.disposed = true;
		this.detachSocket();
	}

	/** Close the socket with its handlers dropped, so the close can't fire onExit. */
	private detachSocket(): void {
		if (this.ws) {
			// Drop handlers before close so the onclose path can't fire onExit.
			this.ws.onmessage = null;
			this.ws.onclose = null;
			this.ws.onerror = null;
			if (this.ws.readyState !== WebSocket.CLOSED) this.ws.close();
		}
		this.ws = null;
	}
}

/**
 * Kill a server-side PTY (used when the user intentionally closes a pane/tab so
 * the PTY doesn't leak on the server, consuming the terminal cap). Best-effort.
 */
export async function deleteServerTerminal(id: string): Promise<void> {
	claimedIds.add(id);
	try {
		const { baseUrl, token } = await resolveServer();
		await fetch(`${baseUrl}/remote/terminals/${id}`, {
			method: 'DELETE',
			headers: authHeaders(token)
		});
	} catch {
		// Server may be gone / already killed — nothing to do.
	}
}
