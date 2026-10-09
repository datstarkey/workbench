/**
 * WS-backed terminal connection for xterm panes.
 *
 * Each xterm pane owns one `TerminalConnection`, attached to the server
 * terminal its pane runs in (`pane.terminalId`). The server's workspace
 * service starts and ends terminals; a pane never creates one. Wire protocol,
 * shared with the mobile client:
 *   client → server  text JSON  {"t":"i","d":…} input
 *                               {"t":"r","c":…,"r":…} resize
 *   server → client  binary     raw PTY bytes (the first frame replays scrollback)
 *                    text JSON  {"t":"takeover"} or {"t":"exit","code":N|null}
 *
 * The server allows one attacher: a second one sends {"t":"takeover"} to the
 * first, which stays detached until the person picks "Take control" — never
 * automatically, or two devices would kick each other back and forth. Closing
 * the socket only detaches; the terminal keeps running.
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

/** Resolved loopback server coordinates. */
export interface ServerInfo {
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

export function resolveServer(): Promise<ServerInfo> {
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

/** Test-only: drop the memoized server-info cache so tests stay isolated. */
export function __resetServerInfoCache(): void {
	serverInfoCache = null;
}

/** One xterm pane's attachment to its server terminal. */
export class TerminalConnection {
	readonly terminalId: string;

	private ws: WebSocket | null = null;
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
	 * @param onReset Called before scrollback replay, so it isn't printed twice.
	 */
	constructor(
		terminalId: string,
		onData: (data: TerminalDataPayload) => void,
		onExit: (info: TerminalExitInfo) => void,
		onReset?: () => void
	) {
		this.terminalId = terminalId;
		this.onData = onData;
		this.onExit = onExit;
		this.onReset = onReset;
	}

	private deliverExit(info: TerminalExitInfo): void {
		if (this.exitDelivered) return;
		this.exitDelivered = true;
		this.onExit(info);
	}

	/** Attach at this size; resolves once the socket is open and sized. */
	async connect(cols: number, rows: number): Promise<void> {
		const { baseUrl, token } = await resolveServer();
		if (this.disposed) return;
		await this.openSocket(terminalWsUrl(baseUrl, this.terminalId, token), cols, rows);
	}

	/** Re-attach after a takeover, kicking whichever device holds the terminal now. */
	async takeControl(cols: number, rows: number): Promise<void> {
		this.detachSocket();
		this.exitDelivered = false;
		await this.connect(cols, rows);
	}

	private openSocket(wsUrl: string, cols: number, rows: number): Promise<void> {
		if (this.disposed) return Promise.resolve();

		// The first binary frame is scrollback replay — reset xterm before it so
		// re-attached history isn't duplicated into the still-mounted terminal.
		let firstFrame = true;

		const ws = new WebSocket(wsUrl);
		ws.binaryType = 'arraybuffer';
		this.ws = ws;

		return new Promise<void>((resolve, reject) => {
			ws.onopen = () => {
				ws.send(JSON.stringify({ t: 'r', c: cols, r: rows }));
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
				// intentional dispose() must stay silent.
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

	/** Detach (close the WebSocket); the terminal keeps running on the server. */
	dispose(): void {
		this.disposed = true;
		this.detachSocket();
	}

	/** Close the socket with its handlers dropped, so the close can't fire onExit. */
	private detachSocket(): void {
		if (this.ws) {
			this.ws.onmessage = null;
			this.ws.onclose = null;
			this.ws.onerror = null;
			if (this.ws.readyState !== WebSocket.CLOSED) this.ws.close();
		}
		this.ws = null;
	}
}
