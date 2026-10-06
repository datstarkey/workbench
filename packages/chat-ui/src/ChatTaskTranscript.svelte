<script lang="ts">
	import { onMount } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import { watch } from 'runed';
	import CheckIcon from '@lucide/svelte/icons/check';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { cn } from '@workbench/ui';
	import type { TaskTranscript } from '@workbench/types';
	import { groupBlocks, stepNames, toolDetail, type StepBlock } from './chat-format';
	import ChatEvent from './ChatEvent.svelte';
	import ChatMarkdown from './ChatMarkdown.svelte';
	import ChatToolCard from './ChatToolCard.svelte';
	import { TaskPoll } from './task-poll.svelte';

	let {
		taskId,
		live,
		cwd,
		fetchTranscript,
		class: className
	}: {
		taskId: string;
		/** Still running: keep refreshing. */
		live: boolean;
		cwd?: string;
		fetchTranscript: (taskId: string) => Promise<TaskTranscript | null>;
		class?: string;
	} = $props();

	const poll = new TaskPoll(
		() => fetchTranscript(taskId),
		() => live,
		2000
	);
	onMount(() => poll.start());
	const blocks = $derived(groupBlocks(poll.value?.items ?? []));

	/** Follow new steps while scrolled to the end; leave the reader alone otherwise. */
	const followEnd: Attachment<HTMLElement> = (node) => {
		let atEnd = true;
		const onScroll = () => {
			atEnd = node.scrollHeight - node.scrollTop - node.clientHeight < 24;
		};
		node.addEventListener('scroll', onScroll, { passive: true });
		watch(
			() => poll.value,
			() => {
				if (atEnd) node.scrollTop = node.scrollHeight;
			}
		);
		return () => node.removeEventListener('scroll', onScroll);
	};
</script>

{#snippet step(block: StepBlock)}
	{#if block.kind === 'quiet'}
		<div class="flex flex-wrap gap-1">
			{#each block.tools as tool (tool.id)}
				<span
					class="max-w-full truncate rounded-md border border-wb-hair px-1.5 py-0.5 font-mono text-[10.5px] text-wb-ink-mute"
					title={toolDetail(tool, cwd)}
				>
					{tool.name} <span class="text-wb-ink">{toolDetail(tool, cwd)}</span>
				</span>
			{/each}
		</div>
	{:else if block.item.kind === 'thinking'}
		{#if block.item.text}
			<details class="text-[11px] text-wb-ink-soft">
				<summary class="w-fit cursor-pointer select-none hover:text-wb-ink-mute">Thinking</summary>
				<p class="mt-1 border-l border-wb-hair pl-2.5 whitespace-pre-wrap">{block.item.text}</p>
			</details>
		{/if}
	{:else if block.item.kind === 'tool'}
		<ChatToolCard tool={block.item} {cwd} />
	{/if}
{/snippet}

<div
	{@attach followEnd}
	class={cn('scrollbar-thin flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto', className)}
>
	{#if !poll.loaded}
		<span class="text-[11px] text-wb-ink-soft">Loading conversation…</span>
	{:else if !poll.value || poll.value.items.length === 0}
		<span class="text-[11px] text-wb-ink-soft">
			{live ? 'No conversation yet.' : 'No conversation to show for this agent.'}
		</span>
	{:else}
		{#if poll.value.start > 0}
			<p class="text-center text-[10.5px] text-wb-ink-soft">Earlier steps aren't shown.</p>
		{/if}
		{#each blocks as block (block.kind === 'item' ? block.item.id : block.id)}
			{#if block.kind === 'steps'}
				<details class="group/steps">
					<summary
						class="steps-summary flex w-fit max-w-full cursor-pointer list-none items-center gap-1.5 rounded-md py-0.5 text-[11px] text-wb-ink-mute select-none hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
					>
						<CheckIcon class="size-3 shrink-0 text-wb-ok" aria-hidden="true" />
						<span class="shrink-0">{block.tools.length} tool calls</span>
						<span class="min-w-0 truncate text-wb-ink-soft">{stepNames(block.tools)}</span>
						<ChevronRightIcon
							class="size-3 shrink-0 text-wb-ink-soft transition-transform group-open/steps:rotate-90"
						/>
					</summary>
					<div class="mt-1.5 flex flex-col gap-1.5 border-l border-wb-hair pl-2.5">
						{#each block.blocks as inner (inner.kind === 'item' ? inner.item.id : inner.id)}
							{@render step(inner)}
						{/each}
					</div>
				</details>
			{:else if block.kind === 'quiet' || block.item.kind === 'thinking' || block.item.kind === 'tool'}
				{@render step(block)}
			{:else if block.item.kind === 'user'}
				<!-- The brief the agent was given; long, so folded to its first line. -->
				<details class="group/brief rounded-lg bg-wb-panel2 px-2.5 py-1.5 text-xs">
					<summary class="flex cursor-pointer gap-1.5 select-none">
						<span class="shrink-0 text-wb-ink-soft">Brief</span>
						<span class="min-w-0 truncate text-wb-ink group-open/brief:hidden">
							{block.item.text}
						</span>
					</summary>
					<p class="mt-1.5 whitespace-pre-wrap text-wb-ink">{block.item.text}</p>
				</details>
			{:else if block.item.kind === 'text'}
				<div class="min-w-0 text-xs">
					<ChatMarkdown text={block.item.text} />
				</div>
			{:else if block.item.kind === 'event'}
				<ChatEvent item={block.item} />
			{:else if block.item.kind === 'notice'}
				<p class="text-center text-[11px] whitespace-pre-wrap text-wb-ink-soft">
					{block.item.text}
				</p>
			{/if}
		{/each}
	{/if}
</div>

<style>
	/* WebKit ignores list-style on summary; the chevron replaces the marker. */
	.steps-summary::-webkit-details-marker {
		display: none;
	}
</style>
