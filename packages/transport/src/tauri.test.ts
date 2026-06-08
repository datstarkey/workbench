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
		const off = await createTauriTransport().subscribe('claude:hook', cb);
		expect(listen).toHaveBeenCalledWith('claude:hook', expect.any(Function));
		expect(cb).toHaveBeenCalledWith({ hello: 'world' });
		expect(off).toBe(unlisten);
	});
});
