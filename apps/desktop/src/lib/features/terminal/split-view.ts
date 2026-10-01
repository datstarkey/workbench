import type { ProjectWorkspace, SplitDirection, TerminalTabState } from '$types/workbench';

const HALVES: Record<SplitDirection, [string, string]> = {
	horizontal: ['0 50% 0 0', '0 0 0 50%'],
	vertical: ['0 0 50% 0', '50% 0 0 0']
};

export interface VisibleSplit {
	direction: SplitDirection;
	/** In tab-strip order: the first takes the left/top half. */
	tabs: [TerminalTabState, TerminalTabState];
}

/**
 * The split to render, or null. A split shows only while the active tab is one
 * of its pair (picking another tab shows that tab alone; picking either half
 * brings the split back) and never for native panes, which are OS views the
 * CSS layout can't place.
 */
export function visibleSplit(ws: ProjectWorkspace): VisibleSplit | null {
	const split = ws.splitView;
	if (
		!split ||
		!(split.direction in HALVES) ||
		ws.renderer === 'native' ||
		!split.tabIds.includes(ws.activeTerminalTabId)
	) {
		return null;
	}
	const tabs = ws.terminalTabs.filter((t) => split.tabIds.includes(t.id));
	return tabs.length === 2 ? { direction: split.direction, tabs: [tabs[0], tabs[1]] } : null;
}

/** CSS `inset` for a tab: its half of the split, or the full area when it isn't in one. */
export function splitInset(split: VisibleSplit | null, tabId: string): string {
	const index = split ? split.tabs.findIndex((t) => t.id === tabId) : -1;
	return split && index !== -1 ? HALVES[split.direction][index] : '0';
}
