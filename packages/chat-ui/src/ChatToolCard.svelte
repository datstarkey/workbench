<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import XIcon from '@lucide/svelte/icons/x';
	import { cn } from '@workbench/ui';
	import {
		formatBytes,
		outputMarkdown,
		patchStats,
		toolDetail,
		type ToolItem
	} from './chat-format';
	import ChatMarkdown from './ChatMarkdown.svelte';
	import Elapsed from './Elapsed.svelte';

	let {
		tool,
		cwd,
		startedAt,
		fetchFullOutput
	}: {
		tool: ToolItem;
		cwd?: string;
		/** When the call was first seen running, for its timer. */
		startedAt?: number;
		fetchFullOutput: (toolId: string) => Promise<string | null>;
	} = $props();

	let open = $state(false);
	/** The whole output, once fetched. */
	let fullOutput = $state<string | null>(null);
	let loadingFull = $state(false);

	async function showAll() {
		loadingFull = true;
		fullOutput = await fetchFullOutput(tool.id);
		loadingFull = false;
	}

	const detail = $derived(toolDetail(tool, cwd));
	const stats = $derived(patchStats(tool.patch));
	const expandable = $derived(Boolean(tool.patch?.length || tool.output));
	const running = $derived(tool.status === 'running');
	const shown = $derived(fullOutput ?? tool.output ?? '');
	const markdown = $derived(outputMarkdown(tool, shown));
</script>

<div
	class={cn(
		'relative overflow-hidden rounded-md border bg-wb-panel',
		tool.status === 'error' ? 'border-wb-err/40' : 'border-wb-hair'
	)}
>
	<button
		type="button"
		class="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-xs enabled:hover:bg-wb-panel2 focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none focus-visible:ring-inset"
		disabled={!expandable}
		aria-expanded={open}
		onclick={() => (open = !open)}
	>
		{#if tool.status === 'ok'}
			<CheckIcon class="size-3.5 shrink-0 text-wb-ok" aria-label="Done" />
		{:else if tool.status === 'error'}
			<XIcon class="size-3.5 shrink-0 text-wb-err" aria-label="Failed" />
		{:else}
			<span class="spinner size-3.5 shrink-0" aria-label="Running"></span>
		{/if}
		<span class="shrink-0 text-wb-ink-mute">{tool.name}</span>
		<span class="min-w-0 flex-1 truncate font-mono text-[11px] text-wb-ink">{detail}</span>
		{#if stats.added || stats.removed}
			<span class="shrink-0 font-mono text-[11px] tabular-nums">
				<span class="text-wb-ok">+{stats.added}</span>
				<span class="text-wb-err">−{stats.removed}</span>
			</span>
		{/if}
		{#if running && startedAt}
			<span class="shrink-0 text-[11px] text-wb-ink-soft"><Elapsed since={startedAt} /></span>
		{/if}
		{#if expandable}
			<ChevronRightIcon
				class={cn('size-3 shrink-0 text-wb-ink-soft transition-transform', open && 'rotate-90')}
			/>
		{/if}
	</button>
	{#if running}
		<span class="sweep" aria-hidden="true"></span>
	{/if}
	{#if open}
		<div
			class="scrollbar-thin max-h-80 overflow-auto border-t border-wb-hair py-1 font-mono text-[11px] leading-relaxed"
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
				{#if markdown !== null}
					<!-- A skill's `# Title` stays card-sized, not chat-sized. -->
					<div
						class="px-2.5 py-1 font-sans text-xs [&_[role=heading]]:text-xs [&_[role=heading]]:font-semibold"
					>
						<ChatMarkdown text={markdown} />
					</div>
				{:else}
					<pre class="px-2.5 whitespace-pre-wrap text-wb-ink-mute">{shown}</pre>
				{/if}
				{#if tool.fullOutputBytes && fullOutput === null}
					<button
						type="button"
						class="mx-2.5 my-1 rounded px-1.5 py-0.5 font-sans text-[11px] text-wb-accent hover:underline focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none disabled:opacity-50"
						disabled={loadingFull}
						onclick={showAll}
					>
						{loadingFull ? 'Loading…' : `Show all (${formatBytes(tool.fullOutputBytes)})`}
					</button>
				{/if}
			{/if}
		</div>
	{/if}
</div>

<style>
	/* A light passing along the card's bottom edge while the tool runs. */
	.sweep {
		position: absolute;
		inset: auto 0 0 0;
		height: 1px;
		background: linear-gradient(
			90deg,
			transparent 0%,
			var(--wb-agent, var(--wb-claude)) 50%,
			transparent 100%
		);
		background-size: 40% 100%;
		background-repeat: no-repeat;
		animation: sweep 1.4s ease-in-out infinite;
	}
	@keyframes sweep {
		from {
			background-position: -40% 0;
		}
		to {
			background-position: 140% 0;
		}
	}
	.spinner {
		border-radius: 999px;
		border: 1.5px solid var(--wb-hair);
		border-top-color: var(--wb-agent, var(--wb-claude));
		animation: spin 0.8s linear infinite;
	}
	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.sweep,
		.spinner {
			animation: none;
		}
		.sweep {
			background: var(--wb-agent, var(--wb-claude));
			opacity: 0.5;
		}
	}
</style>
