<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		title,
		action,
		children
	}: {
		title?: string;
		/** A button beside the title, e.g. "Add". */
		action?: Snippet;
		children: Snippet;
	} = $props();
</script>

<section>
	{#if title || action}
		<div class="mb-2 flex min-h-6 items-center justify-between gap-3 px-0.5">
			<h2 class="text-[11px] font-semibold text-wb-ink-mute">{title}</h2>
			{@render action?.()}
		</div>
	{/if}
	<div class="divide-y divide-wb-hair-soft rounded-lg border border-wb-hair bg-wb-panel2/50">
		{@render children()}
	</div>
</section>
