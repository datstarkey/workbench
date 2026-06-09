import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { InstancesStore, RemoteInstance } from './instances.svelte.ts';

/** Object-backed localStorage stub (this jsdom env's localStorage lacks `clear`). */
function stubLocalStorage() {
	const mem: Record<string, string> = {};
	vi.stubGlobal('localStorage', {
		getItem: (k: string) => (k in mem ? mem[k] : null),
		setItem: (k: string, v: string) => void (mem[k] = String(v)),
		removeItem: (k: string) => void delete mem[k],
		clear: () => {
			for (const k of Object.keys(mem)) delete mem[k];
		}
	});
}

describe('InstancesStore', () => {
	beforeEach(() => {
		stubLocalStorage();
		// add()/setActive() health-ping on a background promise; keep them offline
		// and off the network so the tests don't hit a real server.
		vi.stubGlobal(
			'fetch',
			vi.fn(() => Promise.reject(new Error('no network')))
		);
	});

	afterEach(() => {
		vi.restoreAllMocks();
		vi.unstubAllGlobals();
	});

	it('load() ignores a non-array persisted config instead of crashing', () => {
		localStorage.setItem('workbench.instances', '{"not":"an array"}');
		const store = new InstancesStore();
		expect(() => store.load()).not.toThrow();
		expect(store.remotes).toEqual([]);
		store.dispose();
	});

	it('load() ignores malformed JSON', () => {
		localStorage.setItem('workbench.instances', 'definitely not json{');
		const store = new InstancesStore();
		store.load();
		expect(store.remotes).toEqual([]);
		store.dispose();
	});

	it('load() restores persisted remotes', () => {
		localStorage.setItem(
			'workbench.instances',
			JSON.stringify([{ id: 'a', name: 'box', url: 'http://box:4317' }])
		);
		const store = new InstancesStore();
		store.load();
		expect(store.remotes).toHaveLength(1);
		expect(store.remotes[0].config.name).toBe('box');
		store.dispose();
	});

	it('remove() disposes the removed instance store and resets active to local', () => {
		const store = new InstancesStore();
		const inst = store.add({ name: 'box', url: 'http://box:4317' });
		const disposeSpy = vi.spyOn(inst.store, 'dispose');
		store.setActive(inst.config.id);

		store.remove(inst.config.id);

		expect(disposeSpy).toHaveBeenCalled();
		expect(store.remotes).toHaveLength(0);
		expect(store.activeId).toBe('local');
		store.dispose();
	});

	it('dispose() clears the health poll interval', () => {
		const store = new InstancesStore();
		store.load(); // starts the 15s health poll
		const clearSpy = vi.spyOn(globalThis, 'clearInterval');
		store.dispose();
		expect(clearSpy).toHaveBeenCalled();
	});
});

describe('RemoteInstance.checkHealth', () => {
	afterEach(() => {
		vi.restoreAllMocks();
		vi.unstubAllGlobals();
	});

	it('reloads projects/sessions on every offline→online transition', async () => {
		const inst = new RemoteInstance({ id: '1', name: 'box', url: 'http://box:4317' });
		const refresh = vi.spyOn(inst.store, 'refresh').mockResolvedValue(undefined);

		const online = () =>
			vi.stubGlobal(
				'fetch',
				vi.fn(() => Promise.resolve({ ok: true }))
			);
		const offline = () =>
			vi.stubGlobal(
				'fetch',
				vi.fn(() => Promise.reject(new Error('down')))
			);

		online();
		await inst.checkHealth();
		expect(inst.status).toBe('online');
		expect(refresh).toHaveBeenCalledTimes(1);

		// Still online → no redundant reload.
		await inst.checkHealth();
		expect(refresh).toHaveBeenCalledTimes(1);

		offline();
		await inst.checkHealth();
		expect(inst.status).toBe('offline');
		expect(refresh).toHaveBeenCalledTimes(1);

		// Recovered → reload again (the bug the loadedOnce latch used to skip).
		online();
		await inst.checkHealth();
		expect(inst.status).toBe('online');
		expect(refresh).toHaveBeenCalledTimes(2);
	});
});
