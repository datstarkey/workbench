import { SvelteSet } from 'svelte/reactivity';
import { machineKey } from './machines.svelte.ts';
import { lsGet, lsSet } from './storage.ts';

export const LS_FAVOURITES = 'wb.favourites';
export const LS_COLLAPSED = 'wb.collapsedGroups';

function readSet(key: string): string[] {
	try {
		const parsed: unknown = JSON.parse(lsGet(key) ?? '[]');
		return Array.isArray(parsed) ? parsed.filter((v) => typeof v === 'string') : [];
	} catch {
		return [];
	}
}

/** The home screen's favourite projects and collapsed sections, saved per machine. */
export class ProjectPrefs {
	readonly favourites: SvelteSet<string>;
	readonly collapsed: SvelteSet<string>;

	constructor(private readonly machineId: string) {
		this.favourites = new SvelteSet(readSet(machineKey(LS_FAVOURITES, machineId)));
		this.collapsed = new SvelteSet(readSet(machineKey(LS_COLLAPSED, machineId)));
	}

	toggleFavourite(path: string): void {
		this.toggle(this.favourites, LS_FAVOURITES, path);
	}

	toggleSection(key: string): void {
		this.toggle(this.collapsed, LS_COLLAPSED, key);
	}

	private toggle(set: SvelteSet<string>, key: string, value: string): void {
		if (!set.delete(value)) set.add(value);
		lsSet(machineKey(key, this.machineId), JSON.stringify([...set]));
	}
}
