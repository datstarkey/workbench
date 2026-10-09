/**
 * Terminals opened on another device (e.g. the phone, via server mode) share the
 * loopback server's TerminalManager (and chat sessions its AgentManager), so the
 * desktop can pick them up: this polls the lists and adopts unknown ids into the
 * matching workspace as new tabs.
 */
import type { AgentSummary, ProjectWorkspace, ServerTerminalMeta } from '$types/workbench';

export type AdoptableTerminal = Pick<
	ServerTerminalMeta,
	'id' | 'name' | 'cwd' | 'alive' | 'claudeSessionId' | 'native'
>;

/**
 * Live terminals the desktop neither tracks in a pane nor created/killed itself.
 * A chat's terminal is listed seconds before its chat is, so it's skipped by
 * its Claude session id rather than waiting on the agent list.
 */
export function adoptableTerminals<T extends AdoptableTerminal>(
	list: T[],
	knownIds: ReadonlySet<string>,
	isClaimed: (id: string) => boolean
): T[] {
	return list.filter(
		(t) => t.alive && !t.claudeSessionId && !t.native && !knownIds.has(t.id) && !isClaimed(t.id)
	);
}

/**
 * Live chat sessions the desktop didn't start: `knownIds` holds the session and
 * pane ids of this window's panes (a `/clear` re-key reaches the pane a moment
 * after the server lists it, but the pane id already matches).
 */
export function adoptableChats(
	list: AgentSummary[],
	knownIds: ReadonlySet<string>,
	isClaimed: (sessionId: string) => boolean
): AgentSummary[] {
	return list.filter(
		(c) =>
			!c.exited &&
			!knownIds.has(c.sessionId) &&
			!(c.paneId && knownIds.has(c.paneId)) &&
			!isClaimed(c.sessionId)
	);
}

/**
 * The xterm workspace a terminal running in `cwd` belongs to: a worktree
 * workspace first, then the project's main one. Native-renderer workspaces
 * can't host server terminals. A terminal with no host isn't adopted; a chat
 * gets a background workspace (`WorkspaceStore.adoptServerChat`).
 */
export function adoptionWorkspace(
	workspaces: ProjectWorkspace[],
	cwd: string
): ProjectWorkspace | undefined {
	const hosts = workspaces.filter((w) => w.renderer !== 'native');
	return (
		hosts.find((w) => w.worktreePath === cwd) ??
		hosts.find((w) => !w.worktreePath && w.projectPath === cwd)
	);
}

/**
 * Workspaces without the given panes (and tabs left empty), for persisting:
 * adopted panes point at loopback PTYs that die with the app, so restoring them
 * would reopen as fresh phantom shells.
 */
export function withoutPanes(
	workspaces: ProjectWorkspace[],
	paneIds: ReadonlySet<string>
): ProjectWorkspace[] {
	if (paneIds.size === 0) return workspaces;
	return workspaces.map((ws) => {
		const terminalTabs = ws.terminalTabs
			.map((tab) => ({ ...tab, panes: tab.panes.filter((p) => !paneIds.has(p.id)) }))
			.filter((tab) => tab.panes.length > 0);
		const activeKept = terminalTabs.some((t) => t.id === ws.activeTerminalTabId);
		return {
			...ws,
			terminalTabs,
			activeTerminalTabId: activeKept ? ws.activeTerminalTabId : (terminalTabs[0]?.id ?? '')
		};
	});
}

/** Human-readable server terminal name (shown in other devices' lists) for a pane. */
export function paneDisplayName(
	workspaces: ProjectWorkspace[],
	paneId: string
): string | undefined {
	for (const ws of workspaces) {
		for (const tab of ws.terminalTabs) {
			const index = tab.panes.findIndex((p) => p.id === paneId);
			if (index === -1) continue;
			const suffix = tab.panes.length > 1 ? ` (${index + 1})` : '';
			const where = ws.branch ? `${ws.projectName} [${ws.branch}]` : ws.projectName;
			return `${where} · ${tab.label}${suffix}`;
		}
	}
	return undefined;
}

/** One kind of server session the desktop adopts into tabs. */
export interface AdoptionSource<T> {
	/** Current server items, or null to skip this round. */
	list: () => Promise<T[] | null>;
	/** The listed items not mapped to a pane, released or claimed locally. */
	adoptable: (items: T[]) => T[];
	/** Add a tab for the item; false when no workspace matches. */
	adopt: (item: T) => boolean;
	onAdopted?: (item: T) => void;
}

export type AdoptionRound = () => Promise<void>;

export function adoptionRound<T>(source: AdoptionSource<T>): AdoptionRound {
	return async () => {
		const items = await source.list();
		if (!items) return;
		for (const item of source.adoptable(items)) {
			if (source.adopt(item)) source.onAdopted?.(item);
		}
	};
}

/** Calls made while one is in flight share it: the rounds of one tick fetch once. */
export function shared<T>(fetch: () => Promise<T>): () => Promise<T> {
	let inflight: Promise<T> | null = null;
	return () => (inflight ??= fetch().finally(() => (inflight = null)));
}

export const ADOPTION_POLL_MS = 5000;

/** Runs every source's round every few seconds while the window is visible, and on focus. */
export class AdoptionPoller {
	private timer: ReturnType<typeof setInterval> | null = null;
	private running = false;
	private readonly onFocus = () => void this.tick();

	constructor(
		private readonly rounds: AdoptionRound[],
		private readonly intervalMs = ADOPTION_POLL_MS
	) {}

	start(): void {
		if (this.timer) return;
		this.timer = setInterval(() => void this.tick(), this.intervalMs);
		window.addEventListener('focus', this.onFocus);
		void this.tick();
	}

	async tick(): Promise<void> {
		if (this.running || document.visibilityState !== 'visible') return;
		this.running = true;
		try {
			// One source failing must not cost the others their round.
			await Promise.allSettled(this.rounds.map((round) => round()));
		} finally {
			this.running = false;
		}
	}

	dispose(): void {
		if (this.timer) clearInterval(this.timer);
		this.timer = null;
		window.removeEventListener('focus', this.onFocus);
	}
}
