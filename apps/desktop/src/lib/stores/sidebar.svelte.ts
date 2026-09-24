export type SidebarTab = 'git' | 'github' | 'scripts' | 'boards';

export class SidebarStore {
	activeTab: SidebarTab = $state<SidebarTab>('github');
}
