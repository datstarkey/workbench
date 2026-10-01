import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MobileClient, type TerminalMeta } from './client.svelte.ts';
import { machineKey } from './machines.svelte.ts';
import { jsonResponse, stubLocalStorage, TOKEN } from './test-helpers.ts';

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));

const MAC = 'http://mac:4317';
const PC = 'http://pc:4317';

interface FakeServer {
	terminals: TerminalMeta[];
	/** When set, the next terminal-list GET waits for it. */
	hold?: Promise<void>;
}

/** Fake Workbench servers keyed by host; an unknown host is unreachable. */
function servers(hosts: Record<string, FakeServer>) {
	vi.stubGlobal(
		'fetch',
		vi.fn(async (input: string, init?: RequestInit) => {
			const url = new URL(input);
			const server = hosts[url.hostname];
			if (!server) throw new TypeError('Failed to fetch');
			const method = init?.method ?? 'GET';
			if (url.pathname === '/health') return jsonResponse('ok');
			if (url.pathname === '/remote/terminals' && method === 'POST') {
				const meta = {
					id: `${url.hostname}-t${server.terminals.length + 1}`,
					cwd: '/repo',
					createdAt: 0,
					alive: true
				};
				server.terminals.push(meta);
				return jsonResponse(meta);
			}
			if (url.pathname === '/remote/terminals') {
				const hold = server.hold;
				server.hold = undefined;
				const list = [...server.terminals];
				await hold;
				return jsonResponse(list);
			}
			return jsonResponse([]);
		})
	);
	return hosts;
}

async function connectedTo(url: string, c = new MobileClient()) {
	c.url = url;
	c.token = TOKEN;
	await c.connect();
	expect(c.connectError).toBeNull();
	return c;
}

describe('MobileClient with several machines', () => {
	beforeEach(() => stubLocalStorage());
	afterEach(() => {
		vi.restoreAllMocks();
		vi.unstubAllGlobals();
	});

	it('pairing a second machine adds it instead of replacing the first', async () => {
		servers({ mac: { terminals: [] }, pc: { terminals: [] } });
		const c = await connectedTo(MAC);
		await connectedTo(PC, c);

		expect(c.machines.list.map((m) => m.url)).toEqual([MAC, PC]);
		expect(c.machine?.url).toBe(PC);
		expect(c.machines.activeId).toBe(c.machineId);
	});

	it('switching disposes the old connection, closes its screens and connects to the other machine', async () => {
		servers({ mac: { terminals: [] }, pc: { terminals: [] } });
		const c = await connectedTo(MAC);
		await connectedTo(PC, c);
		const mac = c.machines.list.find((m) => m.url === MAC)!;
		const pcStore = c.store!;
		const dispose = vi.spyOn(pcStore, 'dispose');
		c.openChat({ sessionId: 's', projectPath: '/repo', name: 'repo' });

		await c.switchTo(mac.id);

		expect(dispose).toHaveBeenCalled();
		expect(c.store).not.toBe(pcStore);
		expect(c.store).not.toBeNull();
		expect(c.activeChat).toBeNull();
		expect(c.url).toBe(MAC);
		expect(c.machine?.id).toBe(mac.id);
		expect(new MobileClient().machines.active?.id).toBe(mac.id);
	});

	it('keeps each machine’s terminal ↔ chat links apart', async () => {
		servers({ mac: { terminals: [] }, pc: { terminals: [] } });
		const c = await connectedTo(MAC);
		c.setDefaultView('terminal');
		await c.startClaude('/repo', undefined, 'repo');
		expect(Object.keys(c.claudeTerminals)).toEqual(['mac-t1']);
		const macId = c.machineId!;

		// The PC lists none of the Mac's terminals: its refresh must not prune the Mac's links.
		await connectedTo(PC, c);
		expect(c.claudeTerminals).toEqual({});
		await c.refreshTerminals();

		await c.switchTo(macId);
		expect(Object.keys(c.claudeTerminals)).toEqual(['mac-t1']);
		expect(localStorage.getItem(machineKey('wb.claudeTerminals', macId))).toContain('mac-t1');
	});

	it('drops a terminal list that arrives from the previous machine after switching', async () => {
		let release!: () => void;
		const hosts = servers({ mac: { terminals: [] }, pc: { terminals: [] } });
		const c = await connectedTo(MAC);
		const macId = c.machineId!;
		await connectedTo(PC, c);
		hosts.pc.terminals.push({ id: 'pc-late', cwd: '/repo', createdAt: 0, alive: true });
		hosts.pc.hold = new Promise((r) => (release = r));
		const late = c.refreshTerminals();

		await c.switchTo(macId);
		release();
		await late;

		expect(c.machineId).toBe(macId);
		expect(c.terminals).toEqual([]);
	});

	it('a failed switch leaves the saved machines and the active one alone', async () => {
		servers({ mac: { terminals: [] } });
		const c = await connectedTo(MAC);
		const macId = c.machineId!;
		c.url = 'http://gone:4317';
		c.token = TOKEN;

		await c.connect();

		expect(c.store).toBeNull();
		expect(c.connectError).toMatch(/fetch/i);
		expect(c.machines.list).toHaveLength(1);
		expect(c.machines.activeId).toBe(macId);
	});

	it('forgetting the connected machine disconnects to an empty connect form', async () => {
		servers({ mac: { terminals: [] }, pc: { terminals: [] } });
		const c = await connectedTo(MAC);
		await connectedTo(PC, c);
		const pcStore = c.store!;
		const dispose = vi.spyOn(pcStore, 'dispose');

		c.forget(c.machineId!);

		expect(dispose).toHaveBeenCalled();
		expect(c.store).toBeNull();
		expect(c.url).toBe('');
		expect(c.token).toBe('');
		expect(c.machines.list.map((m) => m.url)).toEqual([MAC]);
		expect(c.hasSavedServer).toBe(false);
	});

	it('forgetting another machine keeps the connection', async () => {
		servers({ mac: { terminals: [] }, pc: { terminals: [] } });
		const c = await connectedTo(MAC);
		const macId = c.machineId!;
		await connectedTo(PC, c);
		const store = c.store;

		c.forget(macId);

		expect(c.store).toBe(store);
		expect(c.machines.list.map((m) => m.url)).toEqual([PC]);
	});

	it('addMachine() leaves the current machine for a blank form but keeps it saved', async () => {
		servers({ mac: { terminals: [] } });
		const c = await connectedTo(MAC);

		c.addMachine();

		expect(c.store).toBeNull();
		expect(c.url).toBe('');
		expect(c.machines.list).toHaveLength(1);
		expect(new MobileClient().hasSavedServer).toBe(true);
	});

	it('marks the machine offline when it stops answering, and online again when it does', async () => {
		const hosts = servers({ mac: { terminals: [] } });
		const c = await connectedTo(MAC);
		expect(c.online).toBe(true);

		const mac = hosts.mac;
		delete hosts.mac;
		await c.refreshTerminals();
		expect(c.online).toBe(false);

		hosts.mac = mac;
		await c.refreshTerminals();
		expect(c.online).toBe(true);
	});
});
