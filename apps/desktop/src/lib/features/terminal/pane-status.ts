import type { TerminalPaneState } from '$types/workbench';

/** The pane has a live server terminal to attach a view to. */
export function attachable(
	pane: TerminalPaneState
): pane is TerminalPaneState & { terminalId: string } {
	return Boolean(pane.terminalId) && (pane.status === 'starting' || pane.status === 'running');
}
