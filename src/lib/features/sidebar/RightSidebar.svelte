<script lang="ts">
	import GithubIcon from '@lucide/svelte/icons/github';
	import LayoutListIcon from '@lucide/svelte/icons/layout-list';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import { getTrelloStore, getWorkbenchSettingsStore } from '$stores/context';
	import GitHubSidebar from '$features/github/GitHubSidebar.svelte';
	import BoardPanel from '$features/trello/BoardPanel.svelte';

	let { onClose }: { onClose: () => void } = $props();

	const trelloStore = getTrelloStore();
	const workbenchSettings = getWorkbenchSettingsStore();

	let trelloEnabled = $derived(workbenchSettings.trelloEnabled);
	let activeTab = $derived(trelloEnabled ? trelloStore.sidebarTab : 'github');
</script>

<div class="flex h-full flex-col border-l border-border/60 bg-background">
	<!-- Tab bar -->
	<div class="flex shrink-0 items-center justify-between border-b border-border/60 px-2 py-1.5">
		<div class="flex items-center gap-0.5">
			<Button
				variant={activeTab === 'github' ? 'secondary' : 'ghost'}
				size="sm"
				class="h-7 gap-1.5 text-xs"
				onclick={() => trelloStore.setSidebarTab('github')}
			>
				<GithubIcon class="size-3.5" />
				GitHub
			</Button>
			{#if trelloEnabled}
				<Button
					variant={activeTab === 'boards' ? 'secondary' : 'ghost'}
					size="sm"
					class="h-7 gap-1.5 text-xs"
					onclick={() => trelloStore.setSidebarTab('boards')}
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
		{#if activeTab === 'github'}
			<GitHubSidebar />
		{:else}
			<BoardPanel />
		{/if}
	</div>
</div>
