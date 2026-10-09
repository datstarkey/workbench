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

/** The pre-machines pairing becomes the first (active) machine. */
function migrateLegacy(): Machine | null {
	const url = lsGet(LEGACY_URL);
	const token = lsGet(LEGACY_TOKEN);
	if (!url || !token) return null;
	return { id: crypto.randomUUID(), name: hostOf(url), url, token };
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
		// Keep the old pairing unless the new list really reached storage.
		if (!readList(lsGet(LS_MACHINES) ?? '').some((m) => m.id === legacy.id)) return;
		lsRemove(LEGACY_URL);
		lsRemove(LEGACY_TOKEN);
	}

	/**
	 * The saved machine at `url` or, failing that, with `token`: tokens are random
	 * per server, so the same token at another address (LAN vs Tailscale) is the same machine.
	 */
	find(url: string, token: string): Machine | undefined {
		return this.list.find((m) => m.url === url) ?? this.list.find((m) => m.token === token);
	}

	/** Pairing: add the machine, or update the matching saved one (`find`); it becomes active. */
	save(url: string, token: string, nickname = ''): Machine {
		const existing = this.find(url, token);
		const machine: Machine = {
			id: existing?.id ?? crypto.randomUUID(),
			name: nickname.trim() || existing?.name || hostOf(url),
			url,
			token
		};
		this.list = existing
			? this.list.map((m) => (m.id === machine.id ? machine : m))
			: [...this.list, machine];
		this.activeId = machine.id;
		this.persist();
		return machine;
	}

	/** Renames unless `name` is blank; returns the name now stored. */
	rename(id: string, name: string): string {
		const trimmed = name.trim();
		if (trimmed) {
			this.list = this.list.map((m) => (m.id === id ? { ...m, name: trimmed } : m));
			this.persist();
		}
		return this.list.find((m) => m.id === id)?.name ?? '';
	}

	/** Forget a machine and everything stored for it. */
	remove(id: string): void {
		this.list = this.list.filter((m) => m.id !== id);
		if (this.activeId === id) this.activeId = null;
		// Remove drafts only for the forgotten machine.
		try {
			const keys = Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i));
			for (const key of keys) if (key?.startsWith(`wb.drafts.${id}.`)) lsRemove(key);
		} catch {
			/* storage may be unavailable */
		}
		this.persist();
	}

	private persist(): void {
		lsSet(LS_MACHINES, JSON.stringify(this.list));
		if (this.activeId) lsSet(LS_ACTIVE, this.activeId);
		else lsRemove(LS_ACTIVE);
	}
}
