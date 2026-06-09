/**
 * TerminalConnection — loopback WebSocket client for xterm panes.
 *
 * Each xterm pane owns one TerminalConnection instance. On connect():
 *  1. Waits for the embedded server to be listening (boot-order gate via a
 *     module-level cached promise over server_status).
 *  2. POSTs /remote/terminals to create (or re-attach to) a server PTY.
 *  3. Opens the WS, sends an initial resize frame, then wires data/exit callbacks.
 *
 * Wire protocol (same as mobile Terminal.svelte):
 *  - client → server TEXT: {"t":"i","d":"..."} input
 *  - client → server TEXT: {"t":"r","c":N,"r":N} resize
 *  - server → client BINARY: raw PTY bytes
 *  - server → client TEXT: {"t":"takeover"} (kicked by another client)
 *  - server → client TEXT: {"t":"exit","code":N} (shell exited)
 *
 * One ordered WS connection preserves input order — no per-session write-chain
 * needed (unlike the old IPC path). dispose() detaches (shell survives for
 * reload-survival); the caller must call deleteTerminal() explicitly for user
 * close / tab close.
 */

import { invoke } from '@tauri-apps/api/core';
import { terminalWsUrl } from '@workbench/transport';

// ── boot-order gate ────────────────────────────────────────────────────────
// The embedded server must be listening before the first terminal attaches.
// We poll server_status (via Tauri IPC) until running=true, then cache the
// result so subsequent panes don't re-poll.

interface ServerStatus {
	running: boolean;
	address: string | null;
}

const POLL_INTERVAL_MS = 100;
const POLL_TIMEOUT_MS = 10_000;

let serverReadyPromise: Promise<{ address: string; token: string | null }> | null = null;

/** Returns the loopback server address once the embedded server is running. */
function waitForServer(): Promise<{ address: string; token: string | null }> {
	if (!serverReadyPromise) {
		serverReadyPromise = pollUntilReady();
	}
	return serverReadyPromise;
}

async function pollUntilReady(): Promise<{ address: string; token: string | null }> {
	const deadline = Date.now() + POLL_TIMEOUT_MS;
	while (Date.now() < deadline) {
		try {
			const status = await invoke<ServerStatus>('server_status');
			if (status.running && status.address) {
				// Loopback connections never need a token (unauthenticated on 127.0.0.1).
				return { address: `http://${status.address}`, token: null };
			}
		} catch {
			// Tauri not ready yet — keep polling.
		}
		await new Promise<void>((r) => setTimeout(r, POLL_INTERVAL_MS));
	}
	throw new Error('[TerminalConnection] Timed out waiting for embedded server to start');
}

// ── types ──────────────────────────────────────────────────────────────────

export interface TerminalMeta {
	id: string;
	name: string | null;
	cwd: string;
	createdAt: number;
	alive: boolean;
}

export interface CreateServerTerminalBody {
	projectPath: string;
	worktreePath?: string;
	name?: string;
	command?: string;
	cols: number;
	rows: number;
}

export type ExitReason = 'taken_over' | 'ended';

export interface ExitEvent {
	reason: ExitReason;
	/** Exit code from the shell, if available. */
	code?: number;
}

// ── TerminalConnection ─────────────────────────────────────────────────────

export class TerminalConnection {
	/** Server-assigned terminal ID. Set after connect() resolves. */
	serverTerminalId: string | null = null;

	onData: ((bytes: Uint8Array) => void) | null = null;
	onExit: ((event: ExitEvent) => void) | null = null;

	private ws: WebSocket | null = null;
	private disposed = false;
	/** Tracks whether we received a control frame before close (to distinguish takeover vs end). */
	private exitControlFrame: ExitEvent | null = null;

	/** @param paneId Client-side pane identity (uid), passed to the server for hook correlation. */
	constructor(private readonly paneId: string) {}

	/**
	 * Connect to the embedded server:
	 * 1. Wait for the server to be ready.
	 * 2. POST /remote/terminals to create a PTY (or pass existingId to re-attach).
	 * 3. Open the WS and send an initial resize frame.
	 */
	async connect(body: CreateServerTerminalBody, existingId?: string): Promise<void> {
		if (this.disposed) return;

		const { address, token } = await waitForServer();

		let meta: TerminalMeta;
		if (existingId) {
			// Try to re-attach to the surviving server terminal.
			try {
				const resp = await fetch(`${address}/remote/terminals/${existingId}`);
				if (resp.ok) {
					meta = (await resp.json()) as TerminalMeta;
				} else {
					// Gone — fall through to create.
					meta = await this.createTerminal(address, token, body);
				}
			} catch {
				meta = await this.createTerminal(address, token, body);
			}
		} else {
			meta = await this.createTerminal(address, token, body);
		}

		if (this.disposed) return;
		this.serverTerminalId = meta.id;

		const url = terminalWsUrl(address, meta.id, token ?? undefined);
		const ws = new WebSocket(url);
		ws.binaryType = 'arraybuffer';
		this.ws = ws;

		ws.onopen = () => {
			if (this.disposed) {
				ws.close();
				return;
			}
			// Send initial resize so the server PTY matches xterm dimensions.
			this.resize(body.cols, body.rows);
		};

		ws.onmessage = (event) => {
			if (this.disposed) return;
			if (event.data instanceof ArrayBuffer) {
				// Binary frame: raw PTY bytes
				this.onData?.(new Uint8Array(event.data));
			} else if (typeof event.data === 'string') {
				// Text control frame
				try {
					const msg = JSON.parse(event.data) as { t?: string; code?: number };
					if (msg.t === 'takeover') {
						this.exitControlFrame = { reason: 'taken_over' };
					} else if (msg.t === 'exit') {
						this.exitControlFrame = { reason: 'ended', code: msg.code };
					}
				} catch {
					// Ignore malformed text frames.
				}
			}
		};

		ws.onclose = () => {
			if (this.disposed) return;
			const event = this.exitControlFrame ?? { reason: 'ended' as const };
			this.onExit?.(event);
		};

		ws.onerror = () => {
			// onclose will fire after onerror — let it handle the exit signal.
		};
	}

	private async createTerminal(
		address: string,
		token: string | null,
		body: CreateServerTerminalBody
	): Promise<TerminalMeta> {
		const headers: Record<string, string> = { 'Content-Type': 'application/json' };
		if (token) headers['Authorization'] = `Bearer ${token}`;

		const payload = {
			projectPath: body.projectPath,
			...(body.worktreePath ? { worktreePath: body.worktreePath } : {}),
			...(body.name ? { name: body.name } : {}),
			...(body.command ? { command: body.command } : {}),
			cols: body.cols,
			rows: body.rows,
			// Pass paneId so the hook bridge can correlate PTY events to workspace panes.
			paneId: this.paneId
		};

		const resp = await fetch(`${address}/remote/terminals`, {
			method: 'POST',
			headers,
			body: JSON.stringify(payload)
		});
		if (!resp.ok) {
			const text = await resp.text().catch(() => resp.statusText);
			throw new Error(`[TerminalConnection] POST /remote/terminals failed: ${text}`);
		}
		return (await resp.json()) as TerminalMeta;
	}

	/** Send input bytes to the PTY. */
	write(data: string): void {
		if (this.ws?.readyState === WebSocket.OPEN) {
			this.ws.send(JSON.stringify({ t: 'i', d: data }));
		}
	}

	/** Notify the server PTY of a resize. */
	resize(cols: number, rows: number): void {
		if (this.ws?.readyState === WebSocket.OPEN) {
			this.ws.send(JSON.stringify({ t: 'r', c: cols, r: rows }));
		}
	}

	/**
	 * Detach (close the WS). The server PTY keeps running — the session survives
	 * a webview reload and can be re-attached. To permanently kill a terminal, call
	 * deleteTerminal() separately.
	 */
	dispose(): void {
		if (this.disposed) return;
		this.disposed = true;
		this.onData = null;
		this.onExit = null;
		if (this.ws) {
			this.ws.onmessage = null;
			this.ws.onclose = null;
			this.ws.onerror = null;
			this.ws.close();
			this.ws = null;
		}
	}
}

/**
 * List all live server terminals. Used by WorkspaceStore for reload-survival
 * reconciliation on startup.
 */
export async function listServerTerminals(): Promise<TerminalMeta[]> {
	const { address, token } = await waitForServer();
	const headers: Record<string, string> = {};
	if (token) headers['Authorization'] = `Bearer ${token}`;
	const resp = await fetch(`${address}/remote/terminals`, { headers });
	if (!resp.ok) return [];
	return (await resp.json()) as TerminalMeta[];
}

/**
 * Delete a server terminal (kills the PTY). Called only on explicit user close.
 */
export async function deleteServerTerminal(id: string): Promise<void> {
	try {
		const { address, token } = await waitForServer();
		const headers: Record<string, string> = {};
		if (token) headers['Authorization'] = `Bearer ${token}`;
		await fetch(`${address}/remote/terminals/${id}`, { method: 'DELETE', headers });
	} catch {
		// Best-effort: the server may have already cleaned up.
	}
}

/** Invalidate the server-ready cache (e.g. when the server is restarted). */
export function resetServerReadyCache(): void {
	serverReadyPromise = null;
}
