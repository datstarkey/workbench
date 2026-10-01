<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		label,
		description,
		control,
		children,
		stack = false
	}: {
		label: string;
		description?: string | Snippet;
		/** Compact controls sit on the right; with `stack`, wide ones go under the text. */
		control?: Snippet;
		/** Notes or extra fields under the row. */
		children?: Snippet;
		stack?: boolean;
	} = $props();
</script>

<div class="px-3.5 py-3">
	<div class={['flex', stack ? 'flex-col gap-2.5' : 'items-center gap-5']}>
		<div class="min-w-0 flex-1">
			<div class="text-[13px] text-wb-ink">{label}</div>
			{#if typeof description === 'string'}
				<p class="mt-0.5 text-xs leading-relaxed text-wb-ink-mute">{description}</p>
			{:else if description}
				<p class="mt-0.5 text-xs leading-relaxed text-wb-ink-mute">{@render description()}</p>
			{/if}
		</div>
		{#if control}
			<div class={stack ? 'min-w-0' : 'flex shrink-0 items-center gap-1.5'}>
				{@render control()}
			</div>
		{/if}
	</div>
	{@render children?.()}
</div>
