import type {
	PaneView,
	ProjectWorkspace,
	TerminalPaneState,
	TerminalTabState
} from '$types/workbench';
import type { ServerWorkspace, WorkspacePane } from '$types/workspace';
import type { WorkspaceUi } from './workspace-ui';

/** A snapshot's workspace as the desktop renders it, with this device's choices. */
export function workspaceView(w: ServerWorkspace, ui: WorkspaceUi): ProjectWorkspace {
	const tabs = w.tabs.map(
		(t): TerminalTabState => ({
			id: t.id,
			// An AI tab shows its session's title once it has one.
			label: (t.kind !== 'shell' && t.panes[0]?.title) || t.label,
			split: t.split,
			type: t.kind,
			panes: t.panes.map((p) => paneView(p, ui))
		})
	);
	const active = ui.activeTabs[w.id];
	return {
		id: w.id,
		projectPath: w.projectPath,
		projectName: w.projectName,
		terminalTabs: tabs,
		activeTerminalTabId: tabs.some((t) => t.id === active) ? active : (tabs[0]?.id ?? ''),
		renderer: w.renderer,
		...(w.worktreePath && { worktreePath: w.worktreePath }),
		...(w.branch && { branch: w.branch }),
		...(w.splitView && { splitView: w.splitView })
	};
}

function paneView(p: WorkspacePane, ui: WorkspaceUi): TerminalPaneState {
	const view: PaneView | undefined =
		p.kind === 'codex'
			? p.codexMode === 'appServer'
				? 'chat'
				: 'terminal'
			: p.kind === 'claude' && ui.chatPanes.includes(p.id)
				? 'chat'
				: undefined;
	return {
		id: p.id,
		type: p.kind,
		...(p.sessionId && { claudeSessionId: p.sessionId }),
		...(p.previousIds?.length && { previousIds: p.previousIds }),
		...(p.accountId !== undefined && { claudeAccountId: p.accountId }),
		...(view && { view }),
		terminalId: p.terminalId,
		status: p.status,
		title: p.title,
		busy: p.busy,
		waiting: p.waiting,
		error: p.error,
		generation: p.generation
	};
}
