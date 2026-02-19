<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import type { GitLogEntry } from '$types/workbench';

	let { entries }: { entries: GitLogEntry[] } = $props();
	let expanded = $state(true);
</script>

<div>
	<button
		type="button"
		class="flex w-full items-center gap-1 px-2 py-1 text-xs font-medium text-muted-foreground hover:text-foreground"
		onclick={() => (expanded = !expanded)}
	>
		{#if expanded}
			<ChevronDownIcon class="size-3" />
		{:else}
			<ChevronRightIcon class="size-3" />
		{/if}
		Recent Commits
	</button>
	{#if expanded}
		<div class="space-y-0.5 px-1">
			{#each entries as entry (entry.sha)}
				<div class="flex items-baseline gap-1.5 rounded px-1.5 py-0.5">
					<span class="shrink-0 font-mono text-[10px] text-muted-foreground">{entry.shortSha}</span>
					<span class="min-w-0 truncate text-xs text-foreground/80" title={entry.message}
						>{entry.message}</span
					>
				</div>
			{/each}
			{#if entries.length === 0}
				<p class="px-1.5 py-1 text-xs text-muted-foreground">No commits yet</p>
			{/if}
		</div>
	{/if}
</div>
