<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import { getGitHubStore } from '$stores/context';

	const githubStore = getGitHubStore();

	let target = $derived(githubStore.sidebarTarget);
	let active = $derived(githubStore.activeTarget);
	let prBranches = $derived(
		target
			? (githubStore.prsByProject[target.projectPath] ?? []).filter(
					(p) => p.state === 'OPEN' && p.headRefName !== active?.branch
				)
			: []
	);

	function show(branch: string) {
		if (target) githubStore.showBranch(target.projectPath, branch);
	}
</script>

{#if target}
	<div
		class="flex h-[34px] shrink-0 items-center gap-1.5 border-b border-wb-hair-soft bg-wb-bg px-3"
	>
		<GitBranchIcon class="size-3 shrink-0 text-wb-ink-soft" />
		<DropdownMenu.Root>
			<DropdownMenu.Trigger
				class="flex min-w-0 items-center gap-1 font-mono text-[11.5px] text-wb-ink hover:text-wb-accent"
				aria-label="Choose branch to view, showing {target.branch}"
			>
				<span class="truncate">{target.branch}</span>
				<ChevronDownIcon class="size-3 shrink-0 text-wb-ink-mute" />
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="start" class="w-64">
				{#if active}
					<DropdownMenu.Label class="text-xs text-muted-foreground"
						>Active workspace</DropdownMenu.Label
					>
					<DropdownMenu.Item onclick={() => githubStore.clearSidebarOverride()}>
						<span class="truncate font-mono text-xs">{active.branch}</span>
					</DropdownMenu.Item>
				{/if}
				{#if prBranches.length > 0}
					<DropdownMenu.Separator />
					<DropdownMenu.Label class="text-xs text-muted-foreground">
						Open pull requests
					</DropdownMenu.Label>
					{#each prBranches as pr (pr.number)}
						<DropdownMenu.Item onclick={() => show(pr.headRefName)}>
							<span class="min-w-0 flex-1 truncate font-mono text-xs">{pr.headRefName}</span>
							<span class="font-mono text-[10.5px] text-muted-foreground">#{pr.number}</span>
						</DropdownMenu.Item>
					{/each}
				{/if}
			</DropdownMenu.Content>
		</DropdownMenu.Root>
		<span class="flex-1"></span>
		{#if githubStore.sidebarOverridden}
			<button
				type="button"
				class="h-[22px] shrink-0 rounded-[5px] bg-wb-accent-soft px-1.5 text-[10.5px] font-medium text-wb-accent hover:bg-wb-accent/25"
				onclick={() => githubStore.clearSidebarOverride()}
			>
				Back to active
			</button>
		{:else}
			<span class="shrink-0 text-[10.5px] text-wb-ink-soft">active workspace</span>
		{/if}
	</div>
{/if}
