<script lang="ts">
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import GithubIcon from '@lucide/svelte/icons/git-pull-request';
	import LayoutListIcon from '@lucide/svelte/icons/layout-list';
	import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '@workbench/ui/button';
	import {
		getGitHubStore,
		getSidebarStore,
		getWorkbenchSettingsStore,
		getWorkspaceStore
	} from '$stores/context';
	import GitHubSidebar from '$features/github/GitHubSidebar.svelte';
	import GitSidebar from '$features/git/GitSidebar.svelte';
	import { checksTone, TONE_BG } from '$features/github/pr-view';
	import BoardPanel from '$features/trello/BoardPanel.svelte';
	import ScriptsPanel from '$features/scripts/ScriptsPanel.svelte';
	import { plural } from '$lib/utils/format';
	import type { SidebarTab } from '$stores/sidebar.svelte';
	import type { Snippet } from 'svelte';

	let { onClose }: { onClose: () => void } = $props();

	const sidebarStore = getSidebarStore();
	const workbenchSettings = getWorkbenchSettingsStore();
	const githubStore = getGitHubStore();
	const workspaceStore = getWorkspaceStore();

	let gitEnabled = $derived(workbenchSettings.gitSidebarEnabled);
	let trelloEnabled = $derived(workbenchSettings.trelloEnabled);

	let changeCount = $derived(workspaceStore.activeGitStatus?.files.length ?? 0);
	let ciTone = $derived(checksTone(githubStore.sidebarChecksOverall));
	const TONE_TITLES = { ok: 'Checks passing', warn: 'Checks running', err: 'Checks failing' };

	// Fall back to 'github' if the selected tab's feature is disabled
	let activeTab = $derived.by((): SidebarTab => {
		const tab = sidebarStore.activeTab;
		if (tab === 'git' && !gitEnabled) return 'github';
		if (tab === 'boards' && !trelloEnabled) return 'github';
		return tab;
	});
</script>

{#snippet changeBadge()}
	<span
		class="flex h-4 min-w-4 items-center justify-center rounded-full bg-wb-accent-soft px-1 font-mono text-[10px] text-wb-accent"
		title={plural(changeCount, 'changed file')}
	>
		{changeCount}
	</span>
{/snippet}

{#snippet checksDot()}
	{#if ciTone}
		<span class={['size-1.5 rounded-full', TONE_BG[ciTone]]} title={TONE_TITLES[ciTone]}></span>
	{/if}
{/snippet}

{#snippet tabButton(tab: SidebarTab, label: string, Icon: typeof GitBranchIcon, badge?: Snippet)}
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
		{@render badge?.()}
	</Button>
{/snippet}

<div class="flex h-full flex-col border-l border-wb-hair bg-wb-panel">
	<!-- Tab bar -->
	<div class="flex h-[38px] shrink-0 items-center justify-between border-b border-wb-hair px-2">
		<div class="flex items-center gap-0.5">
			{#if gitEnabled}
				{@render tabButton('git', 'Git', GitBranchIcon, changeCount > 0 ? changeBadge : undefined)}
			{/if}
			{@render tabButton('github', 'GitHub', GithubIcon, checksDot)}
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
