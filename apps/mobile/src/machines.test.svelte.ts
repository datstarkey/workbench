import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { hostOf, machineKey, normalizeUrl, SavedMachines } from './machines.svelte.ts';
import { stubLocalStorage, TOKEN } from './test-helpers.ts';

const PC_TOKEN = 'pc-token-0123456789abcdef0123456789ab';

describe('normalizeUrl', () => {
	it('adds scheme and default port to a bare host', () => {
		expect(normalizeUrl('100.1.2.3')).toBe('http://100.1.2.3:4317');
	});
	it('keeps an explicit scheme/port and strips a trailing slash', () => {
		expect(normalizeUrl('https://box:9000/')).toBe('https://box:9000');
	});
	it('returns empty for blank input', () => {
		expect(normalizeUrl('   ')).toBe('');
	});
});

describe('hostOf', () => {
	it('names a machine after its host', () => {
		expect(hostOf('http://100.64.1.2:4317')).toBe('100.64.1.2');
		expect(hostOf('https://box.tail1234.ts.net')).toBe('box.tail1234.ts.net');
		expect(hostOf('not a url')).toBe('not a url');
	});
});

describe('SavedMachines', () => {
	beforeEach(() => stubLocalStorage());
	afterEach(() => {
		vi.restoreAllMocks();
		vi.unstubAllGlobals();
	});

	const MAC = 'http://100.64.1.2:4317';
	const PC = 'http://100.64.1.3:4317';

	it('starts empty with nothing saved', () => {
		const m = new SavedMachines();
		expect(m.list).toEqual([]);
		expect(m.active).toBeNull();
	});

	it('migrates the single saved server and drops the old keys', () => {
		localStorage.setItem('wb.serverUrl', MAC);
		localStorage.setItem('wb.token', TOKEN);

		const m = new SavedMachines();

		expect(m.list).toHaveLength(1);
		expect(m.active).toMatchObject({ name: '100.64.1.2', url: MAC, token: TOKEN });
		for (const key of ['wb.serverUrl', 'wb.token']) expect(localStorage.getItem(key)).toBeNull();
		// Persisted: a relaunch sees the same machine, not a second migration.
		expect(new SavedMachines().active).toEqual(m.active);
	});

	it('keeps the old pairing when the migrated list could not be written', () => {
		localStorage.setItem('wb.serverUrl', MAC);
		localStorage.setItem('wb.token', TOKEN);
		const setItem = localStorage.setItem.bind(localStorage);
		vi.spyOn(localStorage, 'setItem').mockImplementation((k: string, v: string) => {
			if (k === 'wb.machines') throw new Error('QuotaExceededError');
			setItem(k, v);
		});

		const m = new SavedMachines();

		expect(m.active?.url).toBe(MAC);
		expect(localStorage.getItem('wb.serverUrl')).toBe(MAC);
		expect(localStorage.getItem('wb.token')).toBe(TOKEN);
	});

	it('does not migrate an address saved without a token', () => {
		localStorage.setItem('wb.serverUrl', MAC);
		expect(new SavedMachines().list).toEqual([]);
	});

	it('save() adds a machine and makes it active', () => {
		const m = new SavedMachines();
		const mac = m.save(MAC, TOKEN);
		const pc = m.save(PC, PC_TOKEN);
		expect(m.list.map((x) => x.url)).toEqual([MAC, PC]);
		expect(m.activeId).toBe(pc.id);
		expect(mac.id).not.toBe(pc.id);
		expect(new SavedMachines().active?.url).toBe(PC);
	});

	it('saves a nickname and keeps it when reconnecting or changing addresses', () => {
		const m = new SavedMachines();
		const desktop = m.save(MAC, TOKEN, '  Desktop  ');
		expect(desktop.name).toBe('Desktop');
		expect(new SavedMachines().active?.name).toBe('Desktop');
		expect(m.save('https://desktop.tail123.ts.net', TOKEN, '  ').name).toBe('Desktop');
		expect(m.save(MAC, TOKEN, 'Office').name).toBe('Office');
		expect(m.list).toHaveLength(1);
		expect(m.active?.id).toBe(desktop.id);
	});

	it('save() updates the token of the machine at the same url instead of adding one', () => {
		const m = new SavedMachines();
		const mac = m.save(MAC, TOKEN);
		m.save(PC, PC_TOKEN);
		m.rename(mac.id, 'MacBook');
		const rotated = 'rotated-token-0123456789abcdef0123456';

		const again = m.save(MAC, rotated);

		expect(m.list).toHaveLength(2);
		expect(again).toEqual({ id: mac.id, name: 'MacBook', url: MAC, token: rotated });
		expect(m.activeId).toBe(mac.id);
	});

	it('save() treats the same token at another address as the same machine', () => {
		const m = new SavedMachines();
		const mac = m.save(MAC, TOKEN);
		m.save(PC, PC_TOKEN);
		const viaMagicDns = 'https://mac.tail1234.ts.net';

		const again = m.save(viaMagicDns, TOKEN);

		expect(m.list).toHaveLength(2);
		expect(again).toMatchObject({ id: mac.id, url: viaMagicDns, token: TOKEN });
		expect(m.find(viaMagicDns, 'other')).toEqual(again);
	});

	it('rename() ignores a blank name', () => {
		const m = new SavedMachines();
		const mac = m.save(MAC, TOKEN);
		m.rename(mac.id, '  ');
		expect(m.active?.name).toBe('100.64.1.2');
		m.rename(mac.id, ' MacBook ');
		expect(new SavedMachines().active?.name).toBe('MacBook');
	});

	it('remove() forgets the machine and its stored state; removing the active one clears it', () => {
		const m = new SavedMachines();
		const mac = m.save(MAC, TOKEN);
		const pc = m.save(PC, PC_TOKEN);
		localStorage.setItem(machineKey('wb.account', pc.id), 'work');

		m.remove(pc.id);

		expect(m.list.map((x) => x.id)).toEqual([mac.id]);
		expect(m.active).toBeNull();
		expect(localStorage.getItem(machineKey('wb.account', pc.id))).toBeNull();
		expect(localStorage.getItem('wb.activeMachine')).toBeNull();
	});

	it('ignores a corrupt saved list', () => {
		localStorage.setItem('wb.machines', '{not json');
		expect(new SavedMachines().list).toEqual([]);
		localStorage.setItem('wb.machines', JSON.stringify([{ id: 1 }, 'x']));
		expect(new SavedMachines().list).toEqual([]);
	});
});
