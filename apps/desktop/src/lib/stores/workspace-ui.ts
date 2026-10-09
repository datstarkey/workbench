import type { ServerWorkspace, WorkspaceLocalSeed } from '$types/workspace';

/**
 * What this device alone decides about the server's workspaces: which one is
 * selected, each one's active tab, and which Claude panes show as chat. Kept
 * in localStorage, keyed by the server's ids.
 */
export interface WorkspaceUi {
	selectedId: string | null;
	/** workspace id → its active tab id. */
	activeTabs: Record<string, string>;
	/** Claude panes shown as chat (Codex's view is its mode, on the server). */
	chatPanes: string[];
}

export const UI_KEY = 'workbench.workspace-ui';

const empty = (): WorkspaceUi => ({ selectedId: null, activeTabs: {}, chatPanes: [] });

/** This device saved its state before (never throws). */
export function hasSavedUi(): boolean {
	try {
		return localStorage.getItem(UI_KEY) !== null;
	} catch {
		return false;
	}
}

/** The state an older desktop saved with the model, as this device's. */
export function seededUi(local: WorkspaceLocalSeed): WorkspaceUi {
	return {
		selectedId: local.selectedId ?? null,
		activeTabs: { ...local.activeTabIds },
		chatPanes: [...(local.chatPanes ?? [])]
	};
}

/** Never throws: storage can be missing, blocked or hold anything. */
export function loadUi(): WorkspaceUi {
	try {
		const raw = JSON.parse(localStorage.getItem(UI_KEY) ?? 'null');
		if (!raw || typeof raw !== 'object') return empty();
		return {
			selectedId: typeof raw.selectedId === 'string' ? raw.selectedId : null,
			activeTabs:
				raw.activeTabs && typeof raw.activeTabs === 'object'
					? Object.fromEntries(
							Object.entries(raw.activeTabs).filter(
								(e): e is [string, string] => typeof e[1] === 'string'
							)
						)
					: {},
			chatPanes: Array.isArray(raw.chatPanes)
				? raw.chatPanes.filter((id: unknown) => typeof id === 'string')
				: []
		};
	} catch {
		return empty();
	}
}

export function saveUi(ui: WorkspaceUi): void {
	try {
		localStorage.setItem(UI_KEY, JSON.stringify(ui));
	} catch {
		/* a per-device convenience: losing it is fine */
	}
}

/**
 * Drop keys whose ids left the snapshot, except `keep` (ids a command just
 * made, which a snapshot may not show yet). Returns null when nothing changed.
 */
export function pruneUi(
	ui: WorkspaceUi,
	workspaces: ServerWorkspace[],
	keep: readonly string[]
): WorkspaceUi | null {
	const ids = new Set(keep);
	for (const w of workspaces) {
		ids.add(w.id);
		for (const t of w.tabs) {
			ids.add(t.id);
			for (const p of t.panes) ids.add(p.id);
		}
	}
	const activeTabs = Object.fromEntries(
		Object.entries(ui.activeTabs).filter(([ws, tab]) => ids.has(ws) && ids.has(tab))
	);
	const chatPanes = ui.chatPanes.filter((id) => ids.has(id));
	const selectedId = ui.selectedId && ids.has(ui.selectedId) ? ui.selectedId : null;
	const changed =
		selectedId !== ui.selectedId ||
		chatPanes.length !== ui.chatPanes.length ||
		Object.keys(activeTabs).length !== Object.keys(ui.activeTabs).length;
	return changed ? { selectedId, activeTabs, chatPanes } : null;
}
