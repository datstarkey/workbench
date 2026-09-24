<script lang="ts">
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import GithubIcon from '@lucide/svelte/icons/git-pull-request';
	import LayoutListIcon from '@lucide/svelte/icons/layout-list';
	import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '@workbench/ui/button';
	import { getSidebarStore, getWorkbenchSettingsStore } from '$stores/context';
	import GitHubSidebar from '$features/github/GitHubSidebar.svelte';
	import GitSidebar from '$features/git/GitSidebar.svelte';
	import BoardPanel from '$features/trello/BoardPanel.svelte';
	import ScriptsPanel from '$features/scripts/ScriptsPanel.svelte';
	import type { SidebarTab } from '$stores/sidebar.svelte';

	let { onClose }: { onClose: () => void } = $props();

	const sidebarStore = getSidebarStore();
	const workbenchSettings = getWorkbenchSettingsStore();

	let gitEnabled = $derived(workbenchSettings.gitSidebarEnabled);
	let trelloEnabled = $derived(workbenchSettings.trelloEnabled);

	// Fall back to 'github' if the selected tab's feature is disabled
	let activeTab = $derived.by((): SidebarTab => {
		const tab = sidebarStore.activeTab;
		if (tab === 'git' && !gitEnabled) return 'github';
		if (tab === 'boards' && !trelloEnabled) return 'github';
		return tab;
	});
</script>

{#snippet tabButton(tab: SidebarTab, label: string, Icon: typeof GitBranchIcon)}
	<Button
		variant="ghost"
		size="sm"
		class={[
			'h-7 gap-1.5 text-xs',
			activeTab === tab
				? 'bg-wb-panel2 text-wb-ink'
				: 'text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink'
		]}
		onclick={() => (sidebarStore.activeTab = tab)}
	>
		<Icon class="size-3.5" />
		{label}
	</Button>
{/snippet}

<div class="flex h-full flex-col border-l border-wb-hair bg-wb-panel">
	<!-- Tab bar -->
	<div class="flex h-[38px] shrink-0 items-center justify-between border-b border-wb-hair px-2">
		<div class="flex items-center gap-0.5">
			{#if gitEnabled}
				{@render tabButton('git', 'Git', GitBranchIcon)}
			{/if}
			{@render tabButton('github', 'GitHub', GithubIcon)}
			{@render tabButton('scripts', 'Scripts', SquareTerminalIcon)}
			{#if trelloEnabled}
				{@render tabButton('boards', 'Boards', LayoutListIcon)}
			{/if}
		</div>
		<Button
			variant="ghost"
			size="icon-sm"
			class="size-6 text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink"
			aria-label="Close sidebar"
			onclick={onClose}
		>
			<XIcon class="size-3" />
		</Button>
	</div>

	<!-- Content -->
	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if activeTab === 'git'}
			<GitSidebar />
		{:else if activeTab === 'github'}
			<GitHubSidebar />
		{:else if activeTab === 'scripts'}
			<ScriptsPanel />
		{:else}
			<BoardPanel />
		{/if}
	</div>
</div>
