/**
 * Terminals opened on another device (e.g. the phone, via server mode) share the
 * loopback server's TerminalManager, so the desktop can pick them up: this polls
 * the list and adopts unknown ids into the matching workspace as new tabs.
 */
import type { ProjectWorkspace } from '$types/workbench';
import type { TerminalMeta } from './terminal-connection';

export type AdoptableTerminal = Pick<TerminalMeta, 'id' | 'name' | 'cwd' | 'alive'>;

/** Live terminals the desktop neither tracks in a pane nor created/killed itself. */
export function adoptableTerminals(
	list: AdoptableTerminal[],
	knownIds: ReadonlySet<string>,
	isClaimed: (id: string) => boolean
): AdoptableTerminal[] {
	return list.filter((t) => t.alive && !knownIds.has(t.id) && !isClaimed(t.id));
}

/**
 * The xterm workspace a terminal running in `cwd` belongs to: a worktree
 * workspace first, then the project's main one. Native-renderer workspaces
 * can't host server terminals, and no workspace is ever created for adoption.
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

export interface AdoptionPollerDeps {
	/** Current server terminals, or null to skip this round. */
	listTerminals: () => Promise<AdoptableTerminal[] | null>;
	isClaimed: (id: string) => boolean;
	/** Server terminal ids already mapped to desktop panes. */
	knownIds: () => Iterable<string>;
	/** Add a tab for the terminal; false when no workspace matches. */
	adopt: (terminal: AdoptableTerminal) => boolean;
	onAdopted?: (terminal: AdoptableTerminal) => void;
	intervalMs?: number;
}

export const ADOPTION_POLL_MS = 5000;

/** Polls every few seconds while the window is visible, and on window focus. */
export class TerminalAdoptionPoller {
	private timer: ReturnType<typeof setInterval> | null = null;
	private running = false;
	private readonly onFocus = () => void this.tick();

	constructor(private readonly deps: AdoptionPollerDeps) {}

	start(): void {
		if (this.timer) return;
		this.timer = setInterval(() => void this.tick(), this.deps.intervalMs ?? ADOPTION_POLL_MS);
		window.addEventListener('focus', this.onFocus);
		void this.tick();
	}

	async tick(): Promise<void> {
		if (this.running || document.visibilityState !== 'visible') return;
		this.running = true;
		try {
			const list = await this.deps.listTerminals();
			if (!list) return;
			const known = new Set(this.deps.knownIds());
			for (const terminal of adoptableTerminals(list, known, this.deps.isClaimed)) {
				if (this.deps.adopt(terminal)) this.deps.onAdopted?.(terminal);
			}
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
