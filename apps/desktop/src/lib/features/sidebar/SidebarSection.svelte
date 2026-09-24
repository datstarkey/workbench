<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import type { Snippet } from 'svelte';
	import { sectionLabel } from './styles';

	let {
		title,
		count,
		open = $bindable(true),
		collapsible = true,
		actions,
		children
	}: {
		title: string;
		count?: number;
		open?: boolean;
		collapsible?: boolean;
		actions?: Snippet;
		children: Snippet;
	} = $props();
</script>

{#snippet label()}
	{title}
	{#if count !== undefined}
		<span class="ml-1 font-mono tracking-normal text-wb-ink-soft">{count}</span>
	{/if}
{/snippet}

<section class="flex flex-col border-t border-wb-hair-soft py-1 first:border-t-0">
	<div class="flex h-7 items-center gap-0.5 pr-2 pl-2.5">
		{#if collapsible}
			<button
				type="button"
				class={['flex flex-1 items-center gap-1 text-left hover:text-wb-ink', sectionLabel]}
				aria-expanded={open}
				onclick={() => (open = !open)}
			>
				{#if open}
					<ChevronDownIcon class="size-3" />
				{:else}
					<ChevronRightIcon class="size-3" />
				{/if}
				{@render label()}
			</button>
		{:else}
			<span class={['flex-1', sectionLabel]}>{@render label()}</span>
		{/if}
		{@render actions?.()}
	</div>
	{#if open || !collapsible}
		{@render children()}
	{/if}
</section>
