import {
	baseName,
	type ServerWorkspace as Workspace,
	type WorkspacePane,
	type WorkspaceTab
} from '@workbench/types';

/** A pane with the tab and workspace it sits in. */
export interface PaneEntry {
	workspace: Workspace;
	tab: WorkspaceTab;
	pane: WorkspacePane;
}

export function paneEntries(workspaces: Workspace[]): PaneEntry[] {
	return workspaces.flatMap((workspace) =>
		workspace.tabs.flatMap((tab) => tab.panes.map((pane) => ({ workspace, tab, pane })))
	);
}

/** The pane running this conversation (also after `/clear` gave it a new id), or this terminal. */
export function findPane(
	entries: PaneEntry[],
	by: { sessionId?: string | null; terminalId?: string | null }
): PaneEntry | null {
	const { sessionId, terminalId } = by;
	return (
		entries.find(
			({ pane }) =>
				(!!terminalId && pane.terminalId === terminalId) ||
				(!!sessionId && (pane.sessionId === sessionId || !!pane.previousIds?.includes(sessionId)))
		) ?? null
	);
}

/** "project" or "project · branch". */
export function workspaceLabel(ws: Workspace): string {
	return ws.worktreePath
		? `${ws.projectName} · ${ws.branch || baseName(ws.worktreePath)}`
		: ws.projectName;
}

export function paneTitle({ tab, pane }: PaneEntry): string {
	return pane.title || tab.label;
}

/** How long a chat keeps re-attaching while its pane's process relaunches. */
const RELAUNCH_WAIT_MS = 30_000;

/**
 * Attach to a pane's session. A restart, rewind or mode switch relaunches the
 * process, and an attach meanwhile finds nothing (404): try again at each
 * change to the host's model while the pane is still there.
 */
export async function attachThroughRelaunch(
	attach: () => Promise<string>,
	paneThere: () => boolean,
	nextChange: (ms: number) => Promise<void>
): Promise<string> {
	const deadline = Date.now() + RELAUNCH_WAIT_MS;
	for (;;) {
		try {
			return await attach();
		} catch (e) {
			const { status } = e as { status?: number };
			const left = deadline - Date.now();
			if (status !== 404 || left <= 0 || !paneThere()) throw e;
			await nextChange(left);
		}
	}
}
