/**
 * Unit tests for TerminalConnection — the WS-backed xterm PTY client.
 *
 * Architecture under test (no real network/PTY):
 *   - serverStatus()        → mocked via tauri-mocks `invokeSpy`
 *   - fetch()               → mocked via `vi.spyOn(global, 'fetch')`
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

// ── Fake WebSocket ────────────────────────────────────────────────────────────

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
		this.onclose?.(new CloseEvent('close', { code, reason }));
	}
}

// ── Module-level setup ────────────────────────────────────────────────────────

// Stub `serverStatus` so `invoke('server_status')` returns a running server
// with a known loopback address.
const SERVER_ADDRESS = '127.0.0.1:59000';

function mockServerRunning(address = SERVER_ADDRESS, token?: string) {
	invokeSpy.mockResolvedValueOnce({ running: true, address, token });
}

// Stub `fetch` so POST /remote/terminals returns a fake terminal meta.
function mockCreateTerminal(id = 'term-abc123'): ReturnType<typeof vi.fn> {
	const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce({
		ok: true,
		json: async () => ({
			id,
			name: null,
			cwd: '/projects/test',
			createdAt: 1_700_000_000_000,
			alive: true
		})
	} as Response);
	return fetchMock;
}

// Replace the global WebSocket with our fake before each test.
let OriginalWebSocket: typeof WebSocket;

beforeEach(() => {
	OriginalWebSocket = globalThis.WebSocket;
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	(globalThis as any).WebSocket = FakeWebSocket;
	FakeWebSocket._last = null;
});

afterEach(() => {
	globalThis.WebSocket = OriginalWebSocket;
	vi.restoreAllMocks();
	clearInvokeMocks();
});

// ── Helpers ───────────────────────────────────────────────────────────────────

const DEFAULT_OPTS = {
	projectPath: '/projects/test',
	cols: 120,
	rows: 30
};

/** Flush all pending microtasks so that async code awaiting resolved Promises proceeds. */
async function flushMicrotasks(times = 5): Promise<void> {
	for (let i = 0; i < times; i++) {
		await Promise.resolve();
	}
}

async function connectAndOpen(overrides?: Partial<typeof DEFAULT_OPTS>) {
	const { TerminalConnection } = await import('./terminal-connection');
	const onData = vi.fn();
	const onExit = vi.fn();
	const onReset = vi.fn();
	const conn = new TerminalConnection(onData, onExit, onReset);

	mockServerRunning();
	mockCreateTerminal();

	const opts = { ...DEFAULT_OPTS, ...overrides };
	const connectPromise = conn.connect(opts);

	// Flush microtasks so that the awaited serverStatus() and fetch() inside
	// connect() resolve and the WebSocket constructor runs before we access _last.
	await flushMicrotasks();

	// Simulate WS reaching OPEN
	const ws = FakeWebSocket._last!;
	ws.openWs();

	await connectPromise;
	return { conn, ws, onData, onExit, onReset };
}

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('TerminalConnection', () => {
	describe('connect()', () => {
		it('invokes server_status to discover the loopback address', async () => {
			mockServerRunning();
			mockCreateTerminal();

			const { TerminalConnection } = await import('./terminal-connection');
			const conn = new TerminalConnection(vi.fn(), vi.fn());
			const p = conn.connect(DEFAULT_OPTS);
			await flushMicrotasks();
			FakeWebSocket._last!.openWs();
			await p;

			expect(invokeSpy).toHaveBeenCalledWith('server_status');
		});

		it('POSTs the correct create-terminal body', async () => {
			mockServerRunning();
			const fetchMock = mockCreateTerminal('t-1');

			const { TerminalConnection } = await import('./terminal-connection');
			const conn = new TerminalConnection(vi.fn(), vi.fn());
			const p = conn.connect({
				projectPath: '/projects/app',
				worktreePath: '/projects/app-wt',
				name: 'My shell',
				command: 'claude',
				cols: 100,
				rows: 25
			});
			await flushMicrotasks();
			FakeWebSocket._last!.openWs();
			await p;

			expect(fetchMock).toHaveBeenCalledWith(
				`http://${SERVER_ADDRESS}/remote/terminals`,
				expect.objectContaining({
					method: 'POST',
					body: JSON.stringify({
						projectPath: '/projects/app',
						worktreePath: '/projects/app-wt',
						name: 'My shell',
						command: 'claude',
						cols: 100,
						rows: 25
					})
				})
			);
		});

		it('opens a WebSocket to the terminal WS endpoint using the returned id', async () => {
			mockServerRunning();
			mockCreateTerminal('term-xyz');

			const { TerminalConnection } = await import('./terminal-connection');
			const conn = new TerminalConnection(vi.fn(), vi.fn());
			const p = conn.connect(DEFAULT_OPTS);
			await flushMicrotasks();
			const ws = FakeWebSocket._last!;
			ws.openWs();
			await p;

			expect(ws.url).toBe(`ws://${SERVER_ADDRESS}/remote/terminals/term-xyz/ws`);
		});

		it('sets binaryType to arraybuffer on the WebSocket', async () => {
			const { conn: _conn, ws } = await connectAndOpen();
			expect(ws.binaryType).toBe('arraybuffer');
		});

		it('stores the server-assigned terminal id after connect', async () => {
			mockServerRunning();
			mockCreateTerminal('stored-id');

			const { TerminalConnection } = await import('./terminal-connection');
			const conn = new TerminalConnection(vi.fn(), vi.fn());
			const p = conn.connect(DEFAULT_OPTS);
			await flushMicrotasks();
			FakeWebSocket._last!.openWs();
			await p;

			expect(conn.terminalId).toBe('stored-id');
		});

		it('appends ?token= when the server exposes a token', async () => {
			mockServerRunning(SERVER_ADDRESS, 'secret-tok');
			mockCreateTerminal('t-tok');

			const { TerminalConnection } = await import('./terminal-connection');
			const conn = new TerminalConnection(vi.fn(), vi.fn());
			const p = conn.connect(DEFAULT_OPTS);
			await flushMicrotasks();
			FakeWebSocket._last!.openWs();
			await p;

			expect(FakeWebSocket._last!.url).toContain('?token=secret-tok');
		});

		it('rejects when the server is not running', async () => {
			invokeSpy.mockResolvedValueOnce({ running: false, address: null });

			const { TerminalConnection } = await import('./terminal-connection');
			const conn = new TerminalConnection(vi.fn(), vi.fn());
			await expect(conn.connect(DEFAULT_OPTS)).rejects.toThrow('embedded server is not running');
		});

		it('rejects when POST /remote/terminals returns an error', async () => {
			mockServerRunning();
			vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce({
				ok: false,
				status: 503
			} as Response);

			const { TerminalConnection } = await import('./terminal-connection');
			const conn = new TerminalConnection(vi.fn(), vi.fn());
			await expect(conn.connect(DEFAULT_OPTS)).rejects.toThrow(
				'POST /remote/terminals failed: 503'
			);
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
			const conn = new TerminalConnection(vi.fn(), vi.fn());

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
			expect(Array.from(onData.mock.calls[0][0] as Uint8Array)).toEqual([0x48, 0x65, 0x6c, 0x6c, 0x6f]);
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
			const conn = new TerminalConnection(vi.fn(), vi.fn());

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
});
