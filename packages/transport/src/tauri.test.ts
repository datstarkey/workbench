import { afterEach, describe, expect, it, vi } from 'vitest';

// Mock the optional @tauri-apps/api peer dep (not installed in this package).
const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen }));

import { createTauriTransport } from './tauri.ts';

describe('TauriTransport', () => {
	afterEach(() => {
		invoke.mockReset();
		listen.mockReset();
		vi.unstubAllGlobals();
	});

	it('sends workspace commands to the loopback listener, looked up once', async () => {
		invoke.mockResolvedValue({ running: true, address: '127.0.0.1:5000', token: 'loop' });
		const fetchSpy = vi.fn(async () => new Response(JSON.stringify({ rev: 1 })));
		vi.stubGlobal('fetch', fetchSpy);
		const transport = createTauriTransport();
		await transport.workspaceCommand({ type: 'closeTab', tabId: 't' });
		await transport.workspaceCommand({ type: 'closeTab', tabId: 'u' });
		expect(invoke).toHaveBeenCalledExactlyOnceWith('terminal_server_status');
		const [url, init] = fetchSpy.mock.calls[0] as unknown as [string, RequestInit];
		expect(url).toBe('http://127.0.0.1:5000/workspace/commands');
		expect((init.headers as Record<string, string>).authorization).toBe('Bearer loop');
	});

	it('retries the loopback lookup after the server was not up yet', async () => {
		invoke.mockResolvedValueOnce({ running: false, address: null, token: null });
		invoke.mockResolvedValueOnce({ running: true, address: '127.0.0.1:5000', token: null });
		vi.stubGlobal(
			'fetch',
			vi.fn(async () => new Response(JSON.stringify({ rev: 1 })))
		);
		const transport = createTauriTransport();
		await expect(transport.workspaceCommand({ type: 'closeTab', tabId: 't' })).rejects.toThrow(
			'not running'
		);
		await expect(transport.workspaceCommand({ type: 'closeTab', tabId: 't' })).resolves.toEqual({
			rev: 1
		});
	});

	it('reports local capabilities (terminal IO + native dialogs)', () => {
		expect(createTauriTransport().capabilities).toEqual({
			terminalIO: true,
			nativeDialogs: true
		});
	});

	it('forwards invoke with the args object when present', async () => {
		invoke.mockResolvedValue(['wt']);
		await createTauriTransport().invoke('list_worktrees', { path: '/repo' });
		expect(invoke).toHaveBeenCalledWith('list_worktrees', { path: '/repo' });
	});

	it('omits the args object entirely when args is undefined (call-shape parity)', async () => {
		invoke.mockResolvedValue([]);
		await createTauriTransport().invoke('list_projects', undefined);
		expect(invoke).toHaveBeenCalledWith('list_projects');
		expect(invoke.mock.calls[0]).toHaveLength(1); // no second argument
	});

	it('subscribe registers a tauri listener and unwraps event.payload', async () => {
		const unlisten = vi.fn();
		listen.mockImplementation((_event: string, handler: (e: { payload: unknown }) => void) => {
			handler({ payload: { hello: 'world' } });
			return Promise.resolve(unlisten);
		});
		const cb = vi.fn();
		const off = await createTauriTransport().subscribe('codex:notify', cb);
		expect(listen).toHaveBeenCalledWith('codex:notify', expect.any(Function));
		expect(cb).toHaveBeenCalledWith({ hello: 'world' });
		expect(off).toBe(unlisten);
	});
});
