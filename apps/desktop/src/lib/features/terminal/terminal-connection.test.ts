/**
 * Unit tests for TerminalConnection — the WS-backed xterm PTY client.
 *
 * Architecture under test (no real network/PTY):
 *   - serverStatus()        → mocked via tauri-mocks `invokeSpy`
 *   - WebSocket             → replaced with a hand-rolled fake that exposes
 *                             `send` spy + manually-triggerable event hooks
 *
 * Test patterns:
 *   - `openWs()` helper    → simulates WS reaching OPEN state
 *   - `recvBinary(bytes)`  → simulates server pushing PTY bytes
 *   - `recvText(json)`     → simulates server pushing a control frame
 *   - `closeWs()`          → simulates WS close
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invokeSpy, clearInvokeMocks } from '../../../test/tauri-mocks';
import { resetLoopbackServer } from '@workbench/transport';

// ── Fake WebSocket ────────────────────────────────────────────────────────────

/** Node only grew a global `CloseEvent` in v23; a minimal stand-in carrying the
 * fields TerminalConnection reads keeps this suite runnable on older runtimes. */
class FakeCloseEvent extends Event {
	readonly code: number;
	readonly reason: string;
	constructor(type: string, init: { code?: number; reason?: string } = {}) {
		super(type);
		this.code = init.code ?? 1000;
		this.reason = init.reason ?? '';
	}
}

/** Minimal WebSocket fake that tracks construction args and exposes event hooks. */
class FakeWebSocket {
	static readonly CONNECTING = 0 as const;
	static readonly OPEN = 1 as const;
	static readonly CLOSING = 2 as const;
	static readonly CLOSED = 3 as const;

	readonly url: string;
	binaryType: BinaryType = 'blob';
	readyState: number = FakeWebSocket.CONNECTING;

	onopen: ((event: Event) => void) | null = null;
	onerror: ((event: Event) => void) | null = null;
	onmessage: ((event: MessageEvent) => void) | null = null;
	onclose: ((event: CloseEvent) => void) | null = null;

	readonly send = vi.fn<(data: string | ArrayBuffer | Blob | ArrayBufferView) => void>();
	readonly close = vi.fn<() => void>().mockImplementation(() => {
		this.readyState = FakeWebSocket.CLOSED;
	});

	constructor(url: string) {
		this.url = url;
		// Register so tests can grab the last-created instance.
		FakeWebSocket._last = this;
	}

	// ── test helpers ──────────────────────────────────────────────────────────

	static _last: FakeWebSocket | null = null;

	/** Simulate the connection reaching OPEN state. */
	openWs(): void {
		this.readyState = FakeWebSocket.OPEN;
		this.onopen?.(new Event('open'));
	}

	/** Simulate the server pushing a binary PTY frame. */
	recvBinary(bytes: Uint8Array): void {
		const buf = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
		this.onmessage?.(new MessageEvent('message', { data: buf }));
	}

	/** Simulate the server pushing a JSON text control frame. */
	recvText(json: Record<string, unknown>): void {
		this.onmessage?.(new MessageEvent('message', { data: JSON.stringify(json) }));
	}

	/** Simulate WS close (detach / shell exited). */
	closeWs(code = 1000, reason = ''): void {
		this.readyState = FakeWebSocket.CLOSED;
		this.onclose?.(new FakeCloseEvent('close', { code, reason }) as CloseEvent);
	}
}

// ── Module-level setup ────────────────────────────────────────────────────────

// Stub `terminalServerStatus` so `invoke('terminal_server_status')` returns a
// running loopback server with a known address.
const SERVER_ADDRESS = '127.0.0.1:59000';

function mockServerRunning(address = SERVER_ADDRESS, token?: string) {
	invokeSpy.mockResolvedValueOnce({ running: true, address, token });
}

// Replace the global WebSocket with our fake before each test.
let OriginalWebSocket: typeof WebSocket;

beforeEach(async () => {
	OriginalWebSocket = globalThis.WebSocket;
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	(globalThis as any).WebSocket = FakeWebSocket;
	FakeWebSocket._last = null;
	// The loopback address is memoized at module scope; reset it so each test
	// re-resolves against its own mockServerRunning() stub.
	resetLoopbackServer();
});

afterEach(() => {
	globalThis.WebSocket = OriginalWebSocket;
	vi.restoreAllMocks();
	clearInvokeMocks();
});

// ── Helpers ───────────────────────────────────────────────────────────────────

/** Flush all pending microtasks so that async code awaiting resolved Promises proceeds. */
async function flushMicrotasks(times = 5): Promise<void> {
	for (let i = 0; i < times; i++) {
		await Promise.resolve();
	}
}

async function connectAndOpen(size = { cols: 120, rows: 30 }, id = 'term-abc123') {
	const { TerminalConnection } = await import('./terminal-connection');
	const onData = vi.fn();
	const onExit = vi.fn();
	const onReset = vi.fn();
	const conn = new TerminalConnection(id, onData, onExit, onReset);

	mockServerRunning();
	const connectPromise = conn.connect(size.cols, size.rows);
	await flushMicrotasks();
	const ws = FakeWebSocket._last!;
	ws.openWs();
	await connectPromise;
	return { conn, ws, onData, onExit, onReset };
}

describe('TerminalConnection', () => {
	describe('connect()', () => {
		it('attaches to its terminal on the loopback server, never creating one', async () => {
			const fetchSpy = vi.spyOn(globalThis, 'fetch');
			const { ws, conn } = await connectAndOpen(undefined, 'srv-1');
			expect(invokeSpy).toHaveBeenCalledWith('terminal_server_status');
			expect(ws.url).toBe(`ws://${SERVER_ADDRESS}/remote/terminals/srv-1/ws`);
			expect(ws.binaryType).toBe('arraybuffer');
			expect(conn.terminalId).toBe('srv-1');
			expect(fetchSpy).not.toHaveBeenCalled();
		});

		it('appends ?token= when the server exposes a token', async () => {
			invokeSpy.mockResolvedValueOnce({ running: true, address: SERVER_ADDRESS, token: 'tok' });
			const { TerminalConnection } = await import('./terminal-connection');
			const conn = new TerminalConnection('t-1', vi.fn(), vi.fn());
			const p = conn.connect(80, 24);
			await flushMicrotasks();
			expect(FakeWebSocket._last!.url).toBe(
				`ws://${SERVER_ADDRESS}/remote/terminals/t-1/ws?token=tok`
			);
			FakeWebSocket._last!.openWs();
			await p;
		});

		it('rejects when the server is not running, and retries the lookup next time', async () => {
			invokeSpy.mockResolvedValueOnce({ running: false, address: null });
			const { TerminalConnection } = await import('./terminal-connection');
			const conn = new TerminalConnection('t-1', vi.fn(), vi.fn());
			await expect(conn.connect(80, 24)).rejects.toThrow('not running');
			const { ws } = await connectAndOpen();
			expect(ws.url).toContain(SERVER_ADDRESS);
		});

		it('keeps the token out of a connection error', async () => {
			invokeSpy.mockResolvedValueOnce({ running: true, address: SERVER_ADDRESS, token: 'secret' });
			const { TerminalConnection } = await import('./terminal-connection');
			const conn = new TerminalConnection('t-1', vi.fn(), vi.fn());
			const p = conn.connect(80, 24);
			await flushMicrotasks();
			FakeWebSocket._last!.onerror?.(new Event('error'));
			await expect(p).rejects.toThrow(/^(?!.*secret).*remote\/terminals\/t-1\/ws/);
		});
	});

	describe('initial resize on open', () => {
		it('sends {t:"r",c,r} immediately after WS opens', async () => {
			const { ws } = await connectAndOpen({ cols: 100, rows: 25 });

			// First send call after open must be the initial resize.
			expect(ws.send).toHaveBeenCalledWith(JSON.stringify({ t: 'r', c: 100, r: 25 }));
		});

		it('sends the resize as the very first message', async () => {
			const { ws } = await connectAndOpen({ cols: 80, rows: 24 });
			expect(ws.send.mock.calls[0][0]).toBe(JSON.stringify({ t: 'r', c: 80, r: 24 }));
		});
	});

	describe('write()', () => {
		it('sends {t:"i",d} when readyState is OPEN', async () => {
			const { conn, ws } = await connectAndOpen();
			ws.send.mockClear();

			conn.write('ls -la\n');

			expect(ws.send).toHaveBeenCalledWith(JSON.stringify({ t: 'i', d: 'ls -la\n' }));
		});

		it('does not send when WebSocket is not OPEN', async () => {
			const { conn, ws } = await connectAndOpen();
			ws.readyState = FakeWebSocket.CLOSING;
			ws.send.mockClear();

			conn.write('hello');

			expect(ws.send).not.toHaveBeenCalled();
		});

		it('does not send before connect() is called', async () => {
			const { TerminalConnection } = await import('./terminal-connection');
			const conn = new TerminalConnection('t', vi.fn(), vi.fn());

			// Should not throw; just silently no-op.
			expect(() => conn.write('data')).not.toThrow();
		});
	});

	describe('resize()', () => {
		it('sends {t:"r",c,r} when readyState is OPEN', async () => {
			const { conn, ws } = await connectAndOpen();
			ws.send.mockClear();

			conn.resize(160, 40);

			expect(ws.send).toHaveBeenCalledWith(JSON.stringify({ t: 'r', c: 160, r: 40 }));
		});

		it('does not send when readyState is not OPEN', async () => {
			const { conn, ws } = await connectAndOpen();
			ws.readyState = FakeWebSocket.CLOSED;
			ws.send.mockClear();

			conn.resize(80, 24);

			expect(ws.send).not.toHaveBeenCalled();
		});
	});

	describe('incoming binary frames', () => {
		it('fires onData with a Uint8Array for each ArrayBuffer frame', async () => {
			const { ws, onData } = await connectAndOpen();
			const bytes = new Uint8Array([0x48, 0x65, 0x6c, 0x6c, 0x6f]); // "Hello"

			ws.recvBinary(bytes);

			expect(onData).toHaveBeenCalledTimes(1);
			expect(onData.mock.calls[0][0]).toBeInstanceOf(Uint8Array);
			expect(Array.from(onData.mock.calls[0][0] as Uint8Array)).toEqual([
				0x48, 0x65, 0x6c, 0x6c, 0x6f
			]);
		});

		it('fires onData for every subsequent binary frame', async () => {
			const { ws, onData } = await connectAndOpen();

			ws.recvBinary(new Uint8Array([1]));
			ws.recvBinary(new Uint8Array([2]));
			ws.recvBinary(new Uint8Array([3]));

			expect(onData).toHaveBeenCalledTimes(3);
		});
	});

	describe('onReset before replay', () => {
		it('calls onReset before delivering the first binary frame', async () => {
			const { ws, onData, onReset } = await connectAndOpen();

			ws.recvBinary(new Uint8Array([0xaa]));

			// onReset must fire before onData for the first frame.
			const resetOrder = onReset.mock.invocationCallOrder[0];
			const dataOrder = onData.mock.invocationCallOrder[0];
			expect(resetOrder).toBeLessThan(dataOrder);
		});

		it('calls onReset exactly once (for the first frame only)', async () => {
			const { ws, onReset } = await connectAndOpen();

			ws.recvBinary(new Uint8Array([1]));
			ws.recvBinary(new Uint8Array([2]));
			ws.recvBinary(new Uint8Array([3]));

			expect(onReset).toHaveBeenCalledTimes(1);
		});

		it('does not call onReset when there are no binary frames', async () => {
			const { ws, onReset } = await connectAndOpen();

			// Only text frames, no binary.
			ws.recvText({ t: 'exit', code: 0 });

			expect(onReset).not.toHaveBeenCalled();
		});
	});

	describe('takeover frame', () => {
		it('fires onExit with reason taken_over on {t:"takeover"}', async () => {
			const { ws, onExit } = await connectAndOpen();

			ws.recvText({ t: 'takeover' });

			expect(onExit).toHaveBeenCalledWith({ reason: 'taken_over' });
		});

		it('does not include a code field for takeover', async () => {
			const { ws, onExit } = await connectAndOpen();
			ws.recvText({ t: 'takeover' });

			expect(onExit.mock.calls[0][0]).not.toHaveProperty('code');
		});
	});

	describe('exit frame', () => {
		it('fires onExit with reason ended and code on {t:"exit",code:0}', async () => {
			const { ws, onExit } = await connectAndOpen();

			ws.recvText({ t: 'exit', code: 0 });

			expect(onExit).toHaveBeenCalledWith({ reason: 'ended', code: 0 });
		});

		it('preserves non-zero exit codes', async () => {
			const { ws, onExit } = await connectAndOpen();

			ws.recvText({ t: 'exit', code: 1 });

			expect(onExit).toHaveBeenCalledWith({ reason: 'ended', code: 1 });
		});
	});

	describe('revoked frame', () => {
		it('ends the session (not a takeover) when the listener is revoked', async () => {
			const { ws, onExit } = await connectAndOpen();
			ws.recvText({ t: 'revoked' });
			ws.closeWs();
			expect(onExit).toHaveBeenCalledTimes(1);
			expect(onExit).toHaveBeenCalledWith({ reason: 'ended' });
		});
	});

	describe('WS close', () => {
		it('fires onExit with reason ended when the socket closes', async () => {
			const { ws, onExit } = await connectAndOpen();

			ws.closeWs();

			expect(onExit).toHaveBeenCalledWith({ reason: 'ended' });
		});

		it('fires onExit even without an explicit exit frame', async () => {
			const { ws, onExit } = await connectAndOpen();
			// Close without any preceding exit text frame.
			ws.closeWs(1001, 'going away');

			expect(onExit).toHaveBeenCalledTimes(1);
		});
	});

	describe('dispose()', () => {
		it('closes the WebSocket', async () => {
			const { conn, ws } = await connectAndOpen();

			conn.dispose();

			expect(ws.close).toHaveBeenCalled();
		});

		it('no-ops when called before connect()', async () => {
			const { TerminalConnection } = await import('./terminal-connection');
			const conn = new TerminalConnection('t', vi.fn(), vi.fn());

			expect(() => conn.dispose()).not.toThrow();
		});

		it('no-ops when called twice', async () => {
			const { conn, ws } = await connectAndOpen();
			conn.dispose();
			// Second call should not throw even though ws is now CLOSED.
			expect(() => conn.dispose()).not.toThrow();
			// close() should have been called at most once (first dispose).
			expect(ws.close).toHaveBeenCalledTimes(1);
		});
	});

	describe('single onExit dispatch', () => {
		it('fires onExit once across the exit frame and the close that follows it', async () => {
			const { ws, onExit } = await connectAndOpen();

			ws.recvText({ t: 'exit', code: 0 });
			ws.closeWs();

			expect(onExit).toHaveBeenCalledTimes(1);
			expect(onExit).toHaveBeenCalledWith({ reason: 'ended', code: 0 });
		});

		it('keeps the takeover reason and does not downgrade to ended on close', async () => {
			const { ws, onExit } = await connectAndOpen();

			ws.recvText({ t: 'takeover' });
			ws.closeWs();

			expect(onExit).toHaveBeenCalledTimes(1);
			expect(onExit).toHaveBeenCalledWith({ reason: 'taken_over' });
		});

		it('stays silent on the close triggered by an intentional dispose()', async () => {
			const { conn, ws, onExit } = await connectAndOpen();

			conn.dispose();
			ws.closeWs();

			expect(onExit).not.toHaveBeenCalled();
		});
	});

	describe('takeControl()', () => {
		it('re-attaches to the same terminal after a takeover', async () => {
			const { conn, ws, onExit } = await connectAndOpen(undefined, 'srv-9');
			ws.recvText({ t: 'takeover' });
			expect(onExit).toHaveBeenCalledWith({ reason: 'taken_over' });

			const p = conn.takeControl(90, 30);
			await flushMicrotasks();
			const next = FakeWebSocket._last!;
			expect(next).not.toBe(ws);
			expect(next.url).toBe(`ws://${SERVER_ADDRESS}/remote/terminals/srv-9/ws`);
			next.openWs();
			await p;
			expect(next.send).toHaveBeenCalledWith(JSON.stringify({ t: 'r', c: 90, r: 30 }));

			next.recvText({ t: 'takeover' });
			expect(onExit).toHaveBeenCalledTimes(2);
		});
	});
});
