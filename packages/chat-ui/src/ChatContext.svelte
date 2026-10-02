<script lang="ts">
	import LayersIcon from '@lucide/svelte/icons/layers';
	import type { TranscriptMeta } from '@workbench/types';
	import { contextUsage } from './usage-format';
	import ChatMetricPill from './ChatMetricPill.svelte';
	let { meta, class: className }: { meta: TranscriptMeta | null; class?: string } = $props();
	const usage = $derived(contextUsage(meta));
</script>

{#if usage && usage.used > 0}
	<ChatMetricPill
		percent={usage.percent}
		title="Context: {usage.used.toLocaleString()} of {usage.limit.toLocaleString()} tokens"
		class={className}
	>
		<LayersIcon class="size-3.5" aria-hidden="true" />
		{#snippet detail()}
			<div class="flex flex-col gap-2">
				<p class="font-semibold">Context window</p>
				<p>{usage.used.toLocaleString()} / {usage.limit.toLocaleString()} tokens</p>
				<p class="text-wb-ink-mute">{Math.round(usage.percent)}% in use</p>
			</div>
		{/snippet}
	</ChatMetricPill>
{/if}
