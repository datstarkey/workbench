/**
 * WS-backed terminal connection for xterm panes.
 *
 * Each xterm pane owns one `TerminalConnection`. It talks to the embedded
 * server's TerminalManager via the same wire protocol as the mobile client:
 *   client → server  text JSON  {"t":"i","d":…} input
 *                               {"t":"r","c":…,"r":…} resize
 *   server → client  binary     raw PTY bytes
 *                    text JSON  {"t":"takeover"} or {"t":"exit","code":N}
 *
 * Boot sequence
 * ─────────────
 * 1. `connect()` calls `serverStatus()` to get the loopback address/token.
 * 2. POST /remote/terminals  → receives { id, …terminalMeta }.
 * 3. Opens WebSocket to ws://<addr>/remote/terminals/<id>/ws[?token=…].
 * 4. On WS `open` sends an initial resize so the PTY starts at the correct size.
 *
 * Single-attach lease
 * ───────────────────
 * The server enforces single-attach: if a second client attaches, the first
 * receives a {"t":"takeover"} frame then its socket is closed. `onExit` is
 * fired with `{ reason: 'taken_over' }` in that case.
 *
 * PTY persistence
 * ───────────────
 * The PTY lives in the server process. Closing the WS (navigate away, webview
 * reload) just detaches — the shell keeps running and can be resumed.
 */

import { serverStatus } from '$lib/server-mode';

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
}

/** Server-side terminal metadata returned by POST /remote/terminals. */
interface TerminalMeta {
	id: string;
	name?: string;
	cwd: string;
	createdAt: number;
	alive: boolean;
}

/**
 * Manages a single WebSocket connection to an embedded-server terminal session.
 *
 * Lifecycle:
 *   1. Construct with `onData` / `onExit` callbacks.
 *   2. Call `connect(opts)` — async, resolves once the WS is open and the
 *      initial resize frame has been sent.
 *   3. Use `write()` / `resize()` to drive the PTY.
 *   4. Call `dispose()` to close the WS (detach — PTY keeps running).
 *   5. Listen for `onExit` to react to shell-exit / takeover.
 */
export class TerminalConnection {
	/** Server-assigned terminal id, available after `connect()` resolves. */
	terminalId: string | null = null;

	private ws: WebSocket | null = null;
	private readonly onData: (data: TerminalDataPayload) => void;
	private readonly onExit: (info: TerminalExitInfo) => void;
	private readonly onReset?: () => void;

	/**
	 * @param onData  Called with raw PTY output bytes as they arrive.
	 * @param onExit  Called when the session ends or is taken over.
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

	/**
	 * Create a server-side PTY and open the WebSocket.
	 *
	 * Resolves once the socket is open and the initial resize frame has been
	 * sent.  Rejects if the server is not running or the POST fails.
	 */
	async connect(opts: ConnectOptions): Promise<void> {
		const status = await serverStatus();
		if (!status.running || !status.address) {
			throw new Error('embedded server is not running');
		}

		const baseUrl = `http://${status.address}`;
		const headers: Record<string, string> = { 'Content-Type': 'application/json' };
		// Token is optional; only loopback connections are used here but we still
		// forward it in case the embedded server was started with one.
		const tokenQuery =
			(status as { token?: string }).token ? `?token=${(status as { token?: string }).token}` : '';

		// Create the terminal on the server.
		const resp = await fetch(`${baseUrl}/remote/terminals`, {
			method: 'POST',
			headers,
			body: JSON.stringify({
				projectPath: opts.projectPath,
				worktreePath: opts.worktreePath ?? null,
				name: opts.name ?? null,
				command: opts.command ?? null,
				cols: opts.cols,
				rows: opts.rows
			})
		});
		if (!resp.ok) {
			throw new Error(`POST /remote/terminals failed: ${resp.status}`);
		}

		const meta: TerminalMeta = await resp.json();
		this.terminalId = meta.id;

		// The first binary frame from the server is scrollback replay.  Call
		// onReset() (which clears the xterm viewport) before that frame arrives
		// so old output isn't duplicated.
		let firstFrame = true;

		// Open the WebSocket attach endpoint.
		const wsUrl = `ws://${status.address}/remote/terminals/${meta.id}/ws${tokenQuery}`;
		const ws = new WebSocket(wsUrl);
		ws.binaryType = 'arraybuffer';
		this.ws = ws;

		return new Promise<void>((resolve, reject) => {
			ws.onopen = () => {
				// Send an initial resize so the PTY starts at the correct dimensions.
				ws.send(JSON.stringify({ t: 'r', c: opts.cols, r: opts.rows }));
				resolve();
			};

			ws.onerror = () => {
				reject(new Error(`WebSocket error connecting to ${wsUrl}`));
			};

			ws.onmessage = (event: MessageEvent) => {
				if (event.data instanceof ArrayBuffer) {
					// Raw PTY output — call onReset before the first (replay) frame.
					if (firstFrame) {
						firstFrame = false;
						this.onReset?.();
					}
					this.onData(new Uint8Array(event.data));
				} else if (typeof event.data === 'string') {
					try {
						const msg = JSON.parse(event.data) as { t: string; code?: number };
						if (msg.t === 'takeover') {
							this.onExit({ reason: 'taken_over' });
						} else if (msg.t === 'exit') {
							this.onExit({ reason: 'ended', code: msg.code });
						}
					} catch {
						// Ignore malformed text frames.
					}
				}
			};

			ws.onclose = () => {
				// Normal WS close (detach / shell exited) — fire onExit so the
				// pane surfaces the end-of-session indicator.
				this.onExit({ reason: 'ended' });
			};
		});
	}

	/**
	 * Send PTY input.  No-op if the socket is not OPEN.
	 */
	write(data: string): void {
		if (this.ws?.readyState === WebSocket.OPEN) {
			this.ws.send(JSON.stringify({ t: 'i', d: data }));
		}
	}

	/**
	 * Send a terminal resize.  No-op if the socket is not OPEN.
	 */
	resize(cols: number, rows: number): void {
		if (this.ws?.readyState === WebSocket.OPEN) {
			this.ws.send(JSON.stringify({ t: 'r', c: cols, r: rows }));
		}
	}

	/**
	 * Detach (close the WebSocket).  The PTY keeps running on the server.
	 */
	dispose(): void {
		if (this.ws && this.ws.readyState !== WebSocket.CLOSED) {
			this.ws.close();
		}
		this.ws = null;
	}
}
