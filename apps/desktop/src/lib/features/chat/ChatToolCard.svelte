<script lang="ts">
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { cn } from '@workbench/ui';
	import type { TranscriptItem } from '$types/workbench';
	import { patchStats, toolDetail } from './chat-format';

	let { tool, cwd }: { tool: Extract<TranscriptItem, { kind: 'tool' }>; cwd?: string } = $props();

	let open = $state(false);

	const detail = $derived(toolDetail(tool, cwd));
	const stats = $derived(patchStats(tool.patch));
	const expandable = $derived(Boolean(tool.patch?.length || tool.output));
</script>

<div
	class={cn(
		'overflow-hidden rounded-md border bg-wb-panel',
		tool.status === 'error' ? 'border-wb-err/40' : 'border-wb-hair'
	)}
>
	<button
		type="button"
		class="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-xs enabled:hover:bg-wb-panel2"
		disabled={!expandable}
		aria-expanded={open}
		onclick={() => (open = !open)}
	>
		<span
			class={cn(
				'size-1.5 shrink-0 rounded-full',
				tool.status === 'running' && 'animate-pulse bg-wb-warn',
				tool.status === 'ok' && 'bg-wb-ok',
				tool.status === 'error' && 'bg-wb-err'
			)}
			aria-label={tool.status}
		></span>
		<span class="shrink-0 text-wb-ink-mute">{tool.name}</span>
		<span class="min-w-0 flex-1 truncate font-mono text-[11px] text-wb-ink">{detail}</span>
		{#if stats.added || stats.removed}
			<span class="shrink-0 font-mono text-[11px] tabular-nums">
				<span class="text-wb-ok">+{stats.added}</span>
				<span class="text-wb-err">−{stats.removed}</span>
			</span>
		{/if}
		{#if expandable}
			<ChevronRightIcon
				class={cn('size-3 shrink-0 text-wb-ink-soft transition-transform', open && 'rotate-90')}
			/>
		{/if}
	</button>
	{#if open}
		<div
			class="max-h-80 overflow-auto border-t border-wb-hair py-1 font-mono text-[11px] leading-relaxed"
		>
			{#if tool.patch?.length}
				{#each tool.patch as hunk, h (h)}
					{#if h > 0}<div class="px-2.5 text-wb-ink-soft">⋯</div>{/if}
					{#each hunk.lines as line, i (i)}
						<div
							class={cn(
								'px-2.5 whitespace-pre',
								line.startsWith('+') && 'bg-wb-ok/10 text-wb-ok',
								line.startsWith('-') && 'bg-wb-err/10 text-wb-err',
								!line.startsWith('+') && !line.startsWith('-') && 'text-wb-ink-mute'
							)}
						>
							{line}
						</div>
					{/each}
				{/each}
			{:else}
				<pre class="px-2.5 whitespace-pre-wrap text-wb-ink-mute">{tool.output}</pre>
			{/if}
		</div>
	{/if}
</div>
