<script lang="ts">
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import GithubIcon from '@lucide/svelte/icons/github';
	import LayoutListIcon from '@lucide/svelte/icons/layout-list';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import { getSidebarStore, getWorkbenchSettingsStore } from '$stores/context';
	import GitHubSidebar from '$features/github/GitHubSidebar.svelte';
	import GitSidebar from '$features/git/GitSidebar.svelte';
	import BoardPanel from '$features/trello/BoardPanel.svelte';
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

<div class="flex h-full flex-col border-l border-border/60 bg-background">
	<!-- Tab bar -->
	<div class="flex shrink-0 items-center justify-between border-b border-border/60 px-2 py-1.5">
		<div class="flex items-center gap-0.5">
			{#if gitEnabled}
				<Button
					variant={activeTab === 'git' ? 'secondary' : 'ghost'}
					size="sm"
					class="h-7 gap-1.5 text-xs"
					onclick={() => sidebarStore.setTab('git')}
				>
					<GitBranchIcon class="size-3.5" />
					Git
				</Button>
			{/if}
			<Button
				variant={activeTab === 'github' ? 'secondary' : 'ghost'}
				size="sm"
				class="h-7 gap-1.5 text-xs"
				onclick={() => sidebarStore.setTab('github')}
			>
				<GithubIcon class="size-3.5" />
				GitHub
			</Button>
			{#if trelloEnabled}
				<Button
					variant={activeTab === 'boards' ? 'secondary' : 'ghost'}
					size="sm"
					class="h-7 gap-1.5 text-xs"
					onclick={() => sidebarStore.setTab('boards')}
				>
					<LayoutListIcon class="size-3.5" />
					Boards
				</Button>
			{/if}
		</div>
		<Button variant="ghost" size="icon-sm" class="size-6" onclick={onClose}>
			<XIcon class="size-3" />
		</Button>
	</div>

	<!-- Content -->
	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if activeTab === 'git'}
			<GitSidebar />
		{:else if activeTab === 'github'}
			<GitHubSidebar />
		{:else}
			<BoardPanel />
		{/if}
	</div>
</div>
