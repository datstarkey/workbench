<script lang="ts">
	import LayersIcon from '@lucide/svelte/icons/layers';
	import Elapsed from './Elapsed.svelte';

	/** `since`: unix ms the compaction started; `tokens`: the context being summarized, if known. */
	let { since, tokens }: { since: number; tokens: number | null } = $props();
</script>

<!-- The engine reports only start and end, so the bar shows activity, not a percentage. -->
<div
	class="flex flex-col gap-1.5 rounded-md border border-wb-hair bg-wb-panel2 px-3 py-1.5 text-xs"
	role="status"
>
	<div class="flex items-center gap-2">
		<LayersIcon class="size-3.5 shrink-0 text-wb-accent" aria-hidden="true" />
		<span class="font-medium text-wb-ink">Compacting conversation</span>
		{#if tokens}
			<span class="text-wb-ink-mute">{Math.round(tokens / 1000)}k tokens</span>
		{/if}
		<span class="ml-auto text-wb-ink-mute"><Elapsed {since} /></span>
	</div>
	<div class="track h-1 overflow-hidden rounded-full bg-wb-hair" aria-hidden="true">
		<div class="bar h-full w-1/3 rounded-full bg-wb-accent"></div>
	</div>
</div>

<style>
	.bar {
		animation: slide 1.4s ease-in-out infinite;
	}
	@keyframes slide {
		from {
			transform: translateX(-100%);
		}
		to {
			transform: translateX(300%);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.bar {
			animation: none;
			width: 100%;
			opacity: 0.4;
		}
	}
</style>
