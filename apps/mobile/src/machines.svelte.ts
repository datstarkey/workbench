import { lsGet, lsRemove, lsSet } from './storage.ts';

/** A Workbench server this phone has paired with. */
export interface Machine {
	id: string;
	name: string;
	url: string;
	token: string;
}

const LS_MACHINES = 'wb.machines';
const LS_ACTIVE = 'wb.activeMachine';
/** The single server saved before machines existed; migrated into the list once. */
const LEGACY_URL = 'wb.serverUrl';
const LEGACY_TOKEN = 'wb.token';
/** Terminal id → the conversation its `claude` runs (terminal ids belong to one machine). */
export const LS_LINKS = 'wb.claudeTerminals';
/** Keys stored per machine as `<key>.<machine id>`. */
const PER_MACHINE = [LS_LINKS];
const DEFAULT_PORT = '4317';

export const machineKey = (key: string, machineId: string) => `${key}.${machineId}`;

// Accept a bare Tailscale IP / host: add http:// and the default port so you can
// just paste the IP.
export function normalizeUrl(raw: string): string {
	let s = raw.trim();
	if (!s) return s;
	if (!/^https?:\/\//.test(s)) s = `http://${s}`;
	try {
		const u = new URL(s);
		if (!u.port) u.port = DEFAULT_PORT;
		return u.toString().replace(/\/$/, '');
	} catch {
		return s.replace(/\/$/, '');
	}
}

export function hostOf(url: string): string {
	try {
		return new URL(url).hostname || url;
	} catch {
		return url;
	}
}

function isMachine(value: unknown): value is Machine {
	const m = value as Machine;
	return (
		!!m &&
		typeof m.id === 'string' &&
		typeof m.name === 'string' &&
		typeof m.url === 'string' &&
		typeof m.token === 'string'
	);
}

function readList(raw: string): Machine[] {
	try {
		const parsed: unknown = JSON.parse(raw);
		return Array.isArray(parsed) ? parsed.filter(isMachine) : [];
	} catch {
		return [];
	}
}

/** The pre-machines pairing becomes the first (active) machine, with its terminal links. */
function migrateLegacy(): Machine | null {
	const url = lsGet(LEGACY_URL);
	const token = lsGet(LEGACY_TOKEN);
	if (!url || !token) return null;
	const machine: Machine = { id: crypto.randomUUID(), name: hostOf(url), url, token };
	const links = lsGet(LS_LINKS);
	if (links !== null) lsSet(machineKey(LS_LINKS, machine.id), links);
	return machine;
}

/** Saved machines plus which one the app connects to; persisted in localStorage. */
export class SavedMachines {
	list = $state<Machine[]>([]);
	activeId = $state<string | null>(null);

	active = $derived(this.list.find((m) => m.id === this.activeId) ?? null);

	constructor() {
		const raw = lsGet(LS_MACHINES);
		if (raw !== null) {
			this.list = readList(raw);
			const id = lsGet(LS_ACTIVE);
			this.activeId = this.list.some((m) => m.id === id) ? id : null;
			return;
		}
		const legacy = migrateLegacy();
		if (!legacy) return;
		this.list = [legacy];
		this.activeId = legacy.id;
		this.persist();
		for (const key of [LEGACY_URL, LEGACY_TOKEN, LS_LINKS]) lsRemove(key);
	}

	/** Pairing: add the machine at `url`, or update its token if already saved; it becomes active. */
	save(url: string, token: string): Machine {
		const existing = this.list.find((m) => m.url === url);
		const machine = existing
			? { ...existing, token }
			: { id: crypto.randomUUID(), name: hostOf(url), url, token };
		this.list = existing
			? this.list.map((m) => (m.id === machine.id ? machine : m))
			: [...this.list, machine];
		this.activeId = machine.id;
		this.persist();
		return machine;
	}

	rename(id: string, name: string): void {
		const trimmed = name.trim();
		if (!trimmed) return;
		this.list = this.list.map((m) => (m.id === id ? { ...m, name: trimmed } : m));
		this.persist();
	}

	/** Forget a machine and everything stored for it. */
	remove(id: string): void {
		this.list = this.list.filter((m) => m.id !== id);
		if (this.activeId === id) this.activeId = null;
		for (const key of PER_MACHINE) lsRemove(machineKey(key, id));
		this.persist();
	}

	private persist(): void {
		lsSet(LS_MACHINES, JSON.stringify(this.list));
		if (this.activeId) lsSet(LS_ACTIVE, this.activeId);
		else lsRemove(LS_ACTIVE);
	}
}
