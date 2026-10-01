<script lang="ts">
	import type { Snippet } from 'svelte';
	import InfoIcon from '@lucide/svelte/icons/info';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';

	let {
		tone = 'info',
		children
	}: {
		/** `info` is a quiet line under a row; `warn` is a boxed callout. */
		tone?: 'info' | 'warn';
		children: Snippet;
	} = $props();
</script>

{#if tone === 'warn'}
	<div
		class="mt-2.5 flex items-start gap-2.5 rounded-md border border-wb-warn/25 bg-wb-warn/8 px-3 py-2.5 text-xs leading-relaxed text-wb-warn"
	>
		<TriangleAlertIcon class="mt-0.5 size-3.5 shrink-0" />
		<span>{@render children()}</span>
	</div>
{:else}
	<div class="mt-2 flex items-start gap-2 text-xs leading-relaxed text-wb-ink-mute">
		<InfoIcon class="mt-0.5 size-3.5 shrink-0" />
		<span>{@render children()}</span>
	</div>
{/if}
