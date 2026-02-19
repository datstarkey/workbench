export type SidebarTab = 'git' | 'github' | 'boards';

export class SidebarStore {
	activeTab: SidebarTab = $state<SidebarTab>('github');

	setTab(tab: SidebarTab) {
		this.activeTab = tab;
	}
}
