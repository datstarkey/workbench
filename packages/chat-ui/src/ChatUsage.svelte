<script lang="ts">
	import { cn } from '@workbench/ui';
	import type { UsageChip } from './chat-format';

	/** Plan limit chips; hover (title) or tap shows when each resets. */
	let { chips, chipClass }: { chips: UsageChip[]; chipClass?: string } = $props();

	let open = $state<string | null>(null);
</script>

{#each chips as chip (chip.label)}
	<button
		type="button"
		class={cn(
			'flex shrink-0 items-center gap-1 tabular-nums focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none',
			chipClass,
			chip.high ? 'text-wb-warn' : 'text-wb-ink-soft'
		)}
		title={chip.title}
		aria-label={chip.title}
		aria-expanded={open === chip.label}
		onclick={() => (open = open === chip.label ? null : chip.label)}
	>
		<span class="text-wb-ink-mute">{chip.label}</span>
		{chip.percent}%
		{#if open === chip.label && chip.resets}
			<span class="text-wb-ink-mute">· resets {chip.resets}</span>
		{/if}
	</button>
{/each}
