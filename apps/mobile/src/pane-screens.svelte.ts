import type { PaneEntry } from './panes.ts';
import type { ClaudeView } from './types.ts';
import type { PaneKind, WorkspacePane } from '@workbench/types';

/** How long a start may take to show its pane before the phone gives up on it. */
export const START_TIMEOUT_MS = 15_000;

/**
 * Which pane's screen is open, Chat or Terminal per Claude pane (this phone's
 * presentation only), and a start the host hasn't shown yet. `reconcile` runs
 * on every change to the host's model.
 */
export class PaneScreens {
	openPaneId = $state<string | null>(null);
	views = $state<Record<string, ClaudeView>>({});
	/** A start in flight: its Starting screen is up and the start buttons are off. */
	starting = $state<{ kind: PaneKind } | null>(null);
	private readonly panes: () => PaneEntry[];
	private readonly onTimeout: () => void;
	/** The open pane has been in the model, so its absence means it left. */
	private shown = false;
	private timer: ReturnType<typeof setTimeout> | null = null;

	constructor(panes: () => PaneEntry[], onTimeout: () => void) {
		this.panes = panes;
		this.onTimeout = onTimeout;
	}

	/** A start begins; false while another is still in flight. */
	begin(kind: PaneKind): boolean {
		if (this.starting) return false;
		this.close();
		this.starting = { kind };
		this.timer = setTimeout(() => {
			this.close();
			this.onTimeout();
		}, START_TIMEOUT_MS);
		return true;
	}

	/** The host answered a start: wait for its pane, or drop the start (null, or left with Back). */
	started(paneId: string | null | undefined, view?: ClaudeView): void {
		if (!this.starting) return;
		if (paneId) this.show(paneId, view);
		else this.close();
	}

	open(paneId: string, view?: ClaudeView): void {
		this.endStart();
		this.show(paneId, view);
	}

	/** Back, a timed-out start, or a pane that left. */
	close(): void {
		this.endStart();
		this.openPaneId = null;
	}

	paneView(pane: WorkspacePane, defaultView: ClaudeView): ClaudeView {
		if (pane.kind === 'shell') return 'terminal';
		if (pane.kind === 'codex') return pane.codexMode === 'appServer' ? 'chat' : 'terminal';
		return this.views[pane.id] ?? defaultView;
	}

	setView(paneId: string, view: ClaudeView): void {
		this.views = { ...this.views, [paneId]: view };
	}

	reconcile(): void {
		const id = this.openPaneId;
		if (id) {
			if (this.panes().some((e) => e.pane.id === id)) {
				this.shown = true;
				this.endStart();
			} else if (this.shown) this.openPaneId = null;
		}
	}

	private show(paneId: string, view?: ClaudeView): void {
		const known = this.panes().map((e) => e.pane.id);
		const views = Object.fromEntries(
			Object.entries(this.views).filter(([id]) => known.includes(id))
		);
		this.views = view ? { ...views, [paneId]: view } : views;
		this.openPaneId = paneId;
		this.shown = false;
		this.reconcile();
	}

	private endStart(): void {
		this.starting = null;
		if (this.timer) clearTimeout(this.timer);
		this.timer = null;
	}
}
