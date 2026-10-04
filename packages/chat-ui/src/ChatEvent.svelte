<script lang="ts">
	import BanIcon from '@lucide/svelte/icons/ban';
	import BrainIcon from '@lucide/svelte/icons/brain';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import ShieldAlertIcon from '@lucide/svelte/icons/shield-alert';
	import WebhookIcon from '@lucide/svelte/icons/webhook';
	import { cn } from '@workbench/ui';
	import type { TranscriptItem } from '@workbench/types';
	import { memoryName } from './artifacts';

	let { item }: { item: Extract<TranscriptItem, { kind: 'event' }> } = $props();

	const ICONS = {
		permissionDenied: ShieldAlertIcon,
		hook: WebhookIcon,
		memory: BrainIcon,
		refusal: BanIcon
	};
	const Icon = $derived(ICONS[item.event] ?? WebhookIcon);
	const files = $derived(item.files ?? []);
	const expandable = $derived(Boolean(item.detail) || files.length > 0);
	const tone = $derived(item.event === 'memory' ? 'text-wb-ink-soft' : 'text-wb-warn');
</script>

{#snippet line()}
	<Icon class={cn('size-3.5 shrink-0', tone)} aria-hidden="true" />
	<span class="min-w-0 truncate">{item.title}</span>
{/snippet}

{#if expandable}
	<details class="group/event text-xs text-wb-ink-mute">
		<summary
			class="event-summary flex w-fit max-w-full cursor-pointer list-none items-center gap-1.5 rounded-md py-0.5 select-none hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
		>
			{@render line()}
			<ChevronRightIcon
				class="size-3 shrink-0 text-wb-ink-soft transition-transform group-open/event:rotate-90 motion-reduce:transition-none"
				aria-hidden="true"
			/>
		</summary>
		<div class="mt-1.5 flex flex-col gap-1.5 border-l border-wb-hair pl-3 text-wb-ink-soft">
			{#if files.length > 0}
				<ul class="flex flex-wrap gap-1.5">
					{#each files as file (file)}
						<li
							class="max-w-full truncate rounded-md border border-wb-hair px-1.5 py-0.5 font-mono text-[11px]"
							title={file}
						>
							{memoryName(file)}
						</li>
					{/each}
				</ul>
			{/if}
			{#if item.detail}
				<p class="scrollbar-thin max-h-48 overflow-y-auto break-words whitespace-pre-wrap">
					{item.detail}
				</p>
			{/if}
		</div>
	</details>
{:else}
	<p class="flex items-center gap-1.5 py-0.5 text-xs text-wb-ink-mute">
		{@render line()}
	</p>
{/if}

<style>
	/* WebKit ignores list-style on summary; the chevron replaces the marker. */
	.event-summary::-webkit-details-marker {
		display: none;
	}
</style>
