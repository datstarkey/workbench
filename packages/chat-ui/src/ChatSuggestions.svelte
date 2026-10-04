<script lang="ts">
	import LightbulbIcon from '@lucide/svelte/icons/lightbulb';

	let {
		suggestions,
		onPick
	}: {
		suggestions: string[];
		/** Put the suggestion in the composer. */
		onPick: (text: string) => void;
	} = $props();
</script>

{#if suggestions.length > 0}
	<div class="suggestions flex flex-wrap gap-1.5" aria-label="Suggested prompts">
		{#each suggestions as suggestion (suggestion)}
			<button
				type="button"
				class="flex max-w-full items-center gap-1.5 rounded-full border border-wb-hair px-3 py-1 text-left text-xs text-wb-ink-mute hover:border-wb-ink-soft hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
				onclick={() => onPick(suggestion)}
			>
				<LightbulbIcon class="size-3 shrink-0 text-wb-ink-soft" aria-hidden="true" />
				<span class="truncate">{suggestion}</span>
			</button>
		{/each}
	</div>
{/if}

<style>
	.suggestions {
		animation: rise 200ms ease-out;
	}
	@keyframes rise {
		from {
			opacity: 0;
			transform: translateY(3px);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.suggestions {
			animation: none;
		}
	}
</style>
