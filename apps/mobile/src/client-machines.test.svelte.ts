import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MobileClient } from './client.svelte.ts';
import { jsonResponse, stubLocalStorage } from './test-helpers.ts';
import type { ServerTerminalMeta as TerminalMeta } from '@workbench/types';

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));

const url = (host: string) => `http://${host}:4317`;
/** Server tokens are random per machine. */
const tokenOf = (host: string) => `${host}-token-0123456789abcdef0123456789`;

interface FakeServer {
	terminals: TerminalMeta[];
	/** `METHOD /path` of every request, in order. */
	calls: string[];
	/** A request whose `METHOD /path` is here waits for the promise before answering. */
	holds: Record<string, Promise<void>>;
}

/** Fake Workbench servers keyed by host; an unknown host is unreachable. */
function servers(...names: string[]) {
	const hosts: Record<string, FakeServer> = {};
	for (const name of names) hosts[name] = { terminals: [], calls: [], holds: {} };
	vi.stubGlobal(
		'fetch',
		vi.fn(async (input: string, init?: RequestInit) => {
			const u = new URL(input);
			const server = hosts[u.hostname];
			if (!server) throw new TypeError('Failed to fetch');
			const method = init?.method ?? 'GET';
			const call = `${method} ${u.pathname}`;
			server.calls.push(call);
			const list = [...server.terminals];
			await server.holds[call];
			if (u.pathname === '/health') return jsonResponse('ok');
			if (u.pathname === '/remote/terminals' && method === 'POST') {
				const meta = {
					id: `${u.hostname}-t${server.terminals.length + 1}`,
					cwd: '/repo',
					createdAt: 0,
					alive: true
				};
				server.terminals.push(meta);
				return jsonResponse(meta);
			}
			if (u.pathname === '/remote/terminals') return jsonResponse(list);
			if (method === 'DELETE') return new Response(null, { status: 204 });
			return jsonResponse([]);
		})
	);
	return hosts;
}

/** A promise to hold a request on, and its release. */
function gate() {
	let release!: () => void;
	const promise = new Promise<void>((r) => (release = r));
	return { promise, release };
}

async function connectedTo(host: string, c = new MobileClient()) {
	c.url = url(host);
	c.token = tokenOf(host);
	await c.connect();
	expect(c.connectError).toBeNull();
	expect(c.notice).toBeNull();
	return c;
}

const idOf = (c: MobileClient, host: string) =>
	c.machines.list.find((m) => m.url === url(host))!.id;

describe('MobileClient with several machines', () => {
	beforeEach(() => stubLocalStorage());
	afterEach(() => {
		vi.restoreAllMocks();
		vi.unstubAllGlobals();
	});

	it('pairs with a nickname and renames a connected host without reconnecting', async () => {
		const hosts = servers('mac');
		const c = new MobileClient();
		c.nickname = '  Desktop  ';
		await connectedTo('mac', c);
		expect(c.machine?.name).toBe('Desktop');
		const store = c.store;
		const connection = c.connection;
		const calls = hosts.mac.calls.length;
		c.machines.rename(c.machineId!, 'Home server');
		expect(c.machine?.name).toBe('Home server');
		expect(c.store).toBe(store);
		expect(c.connection).toBe(connection);
		expect(hosts.mac.calls).toHaveLength(calls);
		expect(new MobileClient().machines.active?.name).toBe('Home server');
		await c.connect();
		expect(c.machine?.name).toBe('Home server');
		c.addMachine();
		expect(c.nickname).toBe('');
	});

	it('a failed pairing does not save a nickname or replace the connected name', async () => {
		servers('mac');
		const c = await connectedTo('mac');
		c.url = url('offline');
		c.token = tokenOf('offline');
		c.nickname = 'Not connected';
		await c.connect();
		expect(c.machines.list).toHaveLength(1);
		expect(c.machine?.name).toBe('mac');
	});

	it('pairing a second machine adds it instead of replacing the first', async () => {
		servers('mac', 'pc');
		const c = await connectedTo('mac');
		await connectedTo('pc', c);

		expect(c.machines.list.map((m) => m.url)).toEqual([url('mac'), url('pc')]);
		expect(c.machine?.url).toBe(url('pc'));
		expect(c.machines.activeId).toBe(c.machineId);
	});

	it('switching drops the old connection, closes its screens and connects to the other machine', async () => {
		servers('mac', 'pc');
		const c = await connectedTo('mac');
		await connectedTo('pc', c);
		const pcStore = c.store!;
		c.openPane('pane');

		await c.switchTo(idOf(c, 'mac'));

		expect(c.store).not.toBe(pcStore);
		expect(c.store).not.toBeNull();
		expect(c.openPaneId).toBeNull();
		expect(c.connection).toEqual({ url: url('mac'), token: tokenOf('mac') });
		expect(c.machine?.url).toBe(url('mac'));
		expect(new MobileClient().machines.active?.url).toBe(url('mac'));
	});

	it('a failed switch keeps the current connection, its screens, and says why', async () => {
		const hosts = servers('mac', 'pc');
		const c = await connectedTo('mac');
		await connectedTo('pc', c);
		const store = c.store!;
		c.openPane('pane');
		delete hosts.mac;

		await c.switchTo(idOf(c, 'mac'));

		expect(c.store).toBe(store);
		expect(c.openPaneId).toBe('pane');
		expect(c.machine?.url).toBe(url('pc'));
		expect(c.machines.active?.url).toBe(url('pc'));
		expect(c.notice).toMatch(/Couldn't switch to mac: .*fetch/i);
		expect(c.connecting).toBe(false);
	});

	it('picking another machine supersedes a connect that is still waiting', async () => {
		const hosts = servers('mac', 'pc', 'nas');
		const c = await connectedTo('mac');
		await connectedTo('pc', c);
		await connectedTo('nas', c);
		const asleep = gate();
		hosts.pc.holds['GET /health'] = asleep.promise;

		const stuck = c.switchTo(idOf(c, 'pc'));
		await vi.waitFor(() => expect(hosts.pc.calls).toContain('GET /health'));
		expect(c.connecting).toBe(true);
		expect(c.connectingTo).toBe(idOf(c, 'pc'));

		await c.switchTo(idOf(c, 'mac'));
		expect(c.machine?.url).toBe(url('mac'));
		expect(c.connecting).toBe(false);

		asleep.release();
		await stuck;
		expect(c.machine?.url).toBe(url('mac'));
		expect(c.connecting).toBe(false);
		expect(c.notice).toBeNull();
	});

	it('forgetting the machine being connected to cancels the connect instead of re-adding it', async () => {
		const hosts = servers('mac', 'pc');
		const c = await connectedTo('mac');
		await connectedTo('pc', c);
		await c.switchTo(idOf(c, 'mac'));
		const slow = gate();
		hosts.pc.holds['GET /health'] = slow.promise;

		const switching = c.switchTo(idOf(c, 'pc'));
		await vi.waitFor(() => expect(hosts.pc.calls).toContain('GET /health'));
		c.forget(idOf(c, 'pc'));
		expect(c.connecting).toBe(false);
		slow.release();
		await switching;

		expect(c.machines.list.map((m) => m.url)).toEqual([url('mac')]);
		expect(c.machine?.url).toBe(url('mac'));
	});

	it('an operation started on one machine never reaches the next one', async () => {
		const hosts = servers('mac', 'pc');
		const c = await connectedTo('mac');
		await connectedTo('pc', c);
		await c.switchTo(idOf(c, 'mac'));
		const creating = gate();
		hosts.mac.holds['POST /remote/terminals'] = creating.promise;

		const opening = c.start('shell', { projectPath: '/repo' });
		await vi.waitFor(() => expect(hosts.mac.calls).toContain('POST /remote/terminals'));
		await c.switchTo(idOf(c, 'pc'));
		const pcCalls = hosts.pc.calls.length;
		creating.release();
		await opening;

		expect(hosts.pc.calls.slice(pcCalls)).toEqual([]);
		expect(c.openPaneId).toBeNull();
		expect(c.panes).toEqual([]);
		expect(c.notice).toBeNull();
	});

	it('drops a terminal list that arrives from the previous machine after switching', async () => {
		const hosts = servers('mac', 'pc');
		const c = await connectedTo('mac');
		await connectedTo('pc', c);
		hosts.pc.terminals.push({ id: 'pc-late', cwd: '/repo', createdAt: 0, alive: true });
		const late = gate();
		hosts.pc.holds['GET /remote/terminals'] = late.promise;
		const refreshing = c.remote!.refresh();

		await c.switchTo(idOf(c, 'mac'));
		late.release();
		await refreshing;

		expect(c.machine?.url).toBe(url('mac'));
		expect(c.panes).toEqual([]);
	});

	it('a failed first connect leaves the saved machines and the active one alone', async () => {
		servers('mac');
		const c = await connectedTo('mac');
		const macId = c.machineId!;
		c.addMachine();
		c.url = url('gone');
		c.token = tokenOf('gone');

		await c.connect();

		expect(c.store).toBeNull();
		expect(c.connectError).toMatch(/fetch/i);
		expect(c.machines.list).toHaveLength(1);
		expect(c.machines.activeId).toBe(macId);
	});

	it('forgetting the connected machine disconnects to an empty connect form', async () => {
		servers('mac', 'pc');
		const c = await connectedTo('mac');
		await connectedTo('pc', c);

		c.forget(c.machineId!);

		expect(c.store).toBeNull();
		expect(c.connection).toBeNull();
		expect(c.url).toBe('');
		expect(c.token).toBe('');
		expect(c.machines.list.map((m) => m.url)).toEqual([url('mac')]);
		expect(c.hasSavedServer).toBe(false);
	});

	it('forgetting another machine keeps the connection', async () => {
		servers('mac', 'pc');
		const c = await connectedTo('mac');
		const macId = c.machineId!;
		await connectedTo('pc', c);
		const store = c.store;

		c.forget(macId);

		expect(c.store).toBe(store);
		expect(c.machines.list.map((m) => m.url)).toEqual([url('pc')]);
	});

	it('addMachine() leaves the current machine for a blank form but keeps it saved', async () => {
		servers('mac');
		const c = await connectedTo('mac');

		c.addMachine();

		expect(c.store).toBeNull();
		expect(c.url).toBe('');
		expect(c.machines.list).toHaveLength(1);
		expect(new MobileClient().hasSavedServer).toBe(true);
	});

	it('re-pairing the same server at another address updates it instead of adding one', async () => {
		servers('mac', 'mac-lan');
		const c = await connectedTo('mac');
		c.url = url('mac-lan');
		c.token = tokenOf('mac');
		await c.connect();

		expect(c.machines.list.map((m) => m.url)).toEqual([url('mac-lan')]);
	});

	it('marks the machine offline when it stops answering, and online again when it does', async () => {
		const hosts = servers('mac');
		const c = await connectedTo('mac');
		expect(c.online).toBe(true);

		const mac = hosts.mac;
		delete hosts.mac;
		await c.remote!.refresh();
		expect(c.online).toBe(false);

		hosts.mac = mac;
		await c.remote!.refresh();
		expect(c.online).toBe(true);
	});
	it('does not restore an old terminal after switching during the post-create refresh', async () => {
		const hosts = servers('mac', 'pc');
		const c = await connectedTo('mac');
		await connectedTo('pc', c);
		await c.switchTo(idOf(c, 'mac'));
		const late = gate();
		hosts.mac.holds['GET /remote/terminals'] = late.promise;
		const opening = c.start('shell', { projectPath: '/repo' });
		await vi.waitFor(() => expect(hosts.mac.calls).toContain('GET /remote/terminals'));
		await c.switchTo(idOf(c, 'pc'));
		late.release();
		await opening;
		expect(c.panes).toEqual([]);
		expect(c.openPaneId).toBeNull();
	});
});
