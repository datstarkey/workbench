<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import PaperclipIcon from '@lucide/svelte/icons/paperclip';
	import RotateCwIcon from '@lucide/svelte/icons/rotate-cw';
	import { cn } from '@workbench/ui';
	import type { AgentChat } from './agent-chat.svelte';
	import ChatActivity from './ChatActivity.svelte';
	import ChatApproval from './ChatApproval.svelte';
	import ChatElicitation from './ChatElicitation.svelte';
	import ChatMarkdown from './ChatMarkdown.svelte';
	import ChatQuestion from './ChatQuestion.svelte';
	import ChatToolCard from './ChatToolCard.svelte';
	import {
		activity,
		agentName,
		groupBlocks,
		stepNames,
		toolDetail,
		type StepBlock
	} from './chat-format';

	let {
		chat,
		cwd,
		projectName,
		onShowTerminal,
		onStarter,
		inlineApprovals = true,
		class: className
	}: {
		chat: AgentChat;
		cwd: string;
		projectName: string;
		/** Absent where the conversation can't open as a terminal (Codex on a phone). */
		onShowTerminal?: () => void;
		/** A starter suggestion was picked; put it in the composer. */
		onStarter: (text: string) => void;
		/** False when the host shows unanswered approvals elsewhere (a phone's bottom sheet). */
		inlineApprovals?: boolean;
		class?: string;
	} = $props();

	const name = $derived(agentName(chat.agent));

	const STARTERS = [
		'Explain how this project is structured',
		'Run the tests and fix what fails',
		'Review my uncommitted changes'
	];

	const blocks = $derived(groupBlocks(chat.items));
	const now = $derived(activity(chat.items, chat.meta));
	const live = $derived(chat.status === 'live');
	const lastId = $derived(chat.items[chat.items.length - 1]?.id);
</script>

{#snippet step(block: StepBlock)}
	{#if block.kind === 'quiet'}
		<div class="flex flex-wrap gap-1.5">
			{#each block.tools as tool (tool.id)}
				<span
					class="max-w-full truncate rounded-md border border-wb-hair px-1.5 py-0.5 font-mono text-[11px] text-wb-ink-mute"
					title={toolDetail(tool, cwd)}
				>
					{tool.name} <span class="text-wb-ink">{toolDetail(tool, cwd)}</span>
				</span>
			{/each}
		</div>
	{:else if block.item.kind === 'thinking'}
		{#if block.item.text}
			<details class="text-xs text-wb-ink-soft">
				<summary class="w-fit cursor-pointer select-none hover:text-wb-ink-mute">Thinking</summary>
				<p class="mt-1.5 border-l border-wb-hair pl-3 whitespace-pre-wrap">
					{block.item.text}
				</p>
			</details>
		{/if}
	{:else if block.item.kind === 'tool'}
		<ChatToolCard
			tool={block.item}
			{cwd}
			startedAt={chat.seenAt[block.item.id]}
			fetchFullOutput={(id) => chat.fullOutput(id)}
		/>
	{/if}
{/snippet}

<!-- `--wb-agent` colours the activity line, tool cards and caret by agent. -->
<div
	class={cn('flex flex-col gap-3.5', className)}
	style:--wb-agent={chat.agent === 'codex' ? 'var(--wb-codex)' : undefined}
>
	{#if chat.status === 'failed'}
		<div class="mx-auto mt-10 flex max-w-md flex-col items-center gap-3 text-center">
			<p class="font-medium">{name} couldn't start</p>
			<p class="text-xs text-wb-ink-mute">{chat.error}</p>
			<div class="flex gap-2">
				<button type="button" class="chat-btn primary" onclick={() => chat.open()}>
					Try again
				</button>
				{#if onShowTerminal}
					<button type="button" class="chat-btn" onclick={onShowTerminal}>Use the terminal</button>
				{/if}
			</div>
		</div>
	{:else if chat.status === 'starting' && chat.items.length === 0}
		<div class="flex flex-col gap-3 pt-2" aria-label="Starting {name}">
			<span class="skeleton h-9 w-2/5 self-end rounded-lg"></span>
			<span class="skeleton h-3 w-4/5 rounded"></span>
			<span class="skeleton h-3 w-3/5 rounded"></span>
			<span class="skeleton h-8 w-full rounded-md"></span>
			<p class="pt-1 text-xs text-wb-ink-soft">Starting {name} in {projectName}…</p>
		</div>
	{:else if chat.items.length === 0 && chat.pending.length === 0}
		<div class="mt-[12vh] flex flex-col gap-4">
			<h2 class="text-lg font-medium text-balance">What should {name} work on?</h2>
			<p class="text-xs break-all text-wb-ink-mute">
				Working in <span class="font-mono text-wb-ink">{cwd}</span>
			</p>
			<div class="flex flex-wrap gap-2">
				{#each STARTERS as starter (starter)}
					<button
						type="button"
						class="rounded-full border border-wb-hair px-3 py-1.5 text-xs text-wb-ink-mute hover:border-wb-ink-soft hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
						onclick={() => onStarter(starter)}
					>
						{starter}
					</button>
				{/each}
			</div>
		</div>
	{/if}

	{#if chat.start > 0}
		<p class="text-center text-xs text-wb-ink-soft">
			{#if onShowTerminal}
				Earlier messages are in the
				<button type="button" class="underline hover:text-wb-ink" onclick={onShowTerminal}
					>terminal</button
				>.
			{:else}
				Earlier messages aren't shown here.
			{/if}
		</p>
	{/if}

	{#each blocks as block (block.kind === 'item' ? block.item.id : block.id)}
		{#if block.kind === 'steps'}
			<details class="group/steps">
				<summary
					class="steps-summary flex w-fit max-w-full cursor-pointer list-none items-center gap-1.5 rounded-md py-0.5 text-xs text-wb-ink-mute select-none hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
				>
					<CheckIcon class="size-3.5 shrink-0 text-wb-ok" aria-hidden="true" />
					<span class="shrink-0">{block.tools.length} tool calls</span>
					<span class="min-w-0 truncate text-wb-ink-soft">{stepNames(block.tools)}</span>
					<ChevronRightIcon
						class="size-3 shrink-0 text-wb-ink-soft transition-transform group-open/steps:rotate-90"
					/>
				</summary>
				<div class="mt-2 flex flex-col gap-2 border-l border-wb-hair pl-3">
					{#each block.blocks as inner (inner.kind === 'item' ? inner.item.id : inner.id)}
						{@render step(inner)}
					{/each}
				</div>
			</details>
		{:else if block.kind === 'quiet' || block.item.kind === 'thinking' || block.item.kind === 'tool'}
			{@render step(block)}
		{:else if block.item.kind === 'user'}
			{@const previews = chat.imagePreviews[block.item.id] ?? []}
			<div class="flex max-w-[85%] flex-col items-end gap-1.5 self-end">
				{#if previews.length > 0}
					<div class="flex flex-wrap justify-end gap-1.5">
						{#each previews as src, i (i)}
							<img
								{src}
								alt="Attached"
								class="size-20 rounded-lg border border-wb-hair object-cover"
							/>
						{/each}
					</div>
				{:else if block.item.images}
					<span class="rounded-md border border-wb-hair px-2 py-0.5 text-[11px] text-wb-ink-mute">
						{block.item.images === 1 ? '1 image' : `${block.item.images} images`}
					</span>
				{/if}
				{@render attachedFiles(block.item.files ?? [])}
				{#if block.item.text}
					<div class="rounded-2xl rounded-br-md bg-wb-panel2 px-3.5 py-2 whitespace-pre-wrap">
						{block.item.text}
					</div>
				{/if}
			</div>
		{:else if block.item.kind === 'text'}
			<div class="flex min-w-0 flex-col gap-2">
				<ChatMarkdown text={block.item.text} />
				{#if now.kind === 'writing' && block.item.id === lastId}
					<span class="caret" aria-hidden="true"></span>
				{/if}
			</div>
		{:else if block.item.kind === 'approval'}
			{@const approval = block.item}
			{#if inlineApprovals || approval.decision || approval.expired}
				{#if approval.tool === 'AskUserQuestion'}
					<ChatQuestion
						{approval}
						agent={chat.agent}
						onAnswer={(decision, answers) => chat.approve(approval.id, decision, answers)}
					/>
				{:else}
					<ChatApproval
						{approval}
						agent={chat.agent}
						{cwd}
						onDecide={(decision) => chat.approve(approval.id, decision)}
					/>
				{/if}
			{/if}
		{:else if block.item.kind === 'elicitation'}
			{@const elicitation = block.item}
			{#if inlineApprovals || elicitation.action || elicitation.expired}
				<ChatElicitation
					{elicitation}
					agent={chat.agent}
					onAnswer={(action, content) => chat.elicit(elicitation.id, action, content)}
				/>
			{/if}
		{:else}
			<p class="text-center text-xs whitespace-pre-wrap text-wb-ink-soft">
				{block.item.text}
			</p>
		{/if}
	{/each}

	{#each chat.pending as prompt (prompt.id)}
		<div class="pending flex max-w-[85%] flex-col items-end gap-1.5 self-end">
			{#if prompt.previews.length > 0}
				<div class="flex flex-wrap justify-end gap-1.5">
					{#each prompt.previews as src, i (i)}
						<img
							{src}
							alt="Attached"
							class="size-20 rounded-lg border border-wb-hair object-cover"
						/>
					{/each}
				</div>
			{/if}
			{@render attachedFiles(prompt.files)}
			{#if prompt.text}
				<div class="rounded-2xl rounded-br-md bg-wb-panel2 px-3.5 py-2 whitespace-pre-wrap">
					{prompt.text}
				</div>
			{/if}
		</div>
	{/each}

	{#if now.kind !== 'idle' && live}
		<ChatActivity activity={now} since={chat.busySince} {cwd} onStop={() => chat.interrupt()} />
	{/if}

	{#if chat.status === 'exited'}
		<div class="flex items-center gap-3 rounded-lg border border-wb-hair px-3.5 py-2.5 text-xs">
			<span class="flex-1 text-wb-ink-mute">
				{chat.error ?? `${name} stopped. The conversation is saved.`}
			</span>
			<button type="button" class="chat-btn" onclick={() => chat.open()}>
				<RotateCwIcon class="size-3" /> Restart
			</button>
		</div>
	{/if}
</div>

{#snippet attachedFiles(names: string[])}
	{#if names.length > 0}
		<ul class="flex flex-wrap justify-end gap-1.5" aria-label="Attached files">
			{#each names as name, i (i)}
				<li
					class="flex max-w-56 items-center gap-1 rounded-md border border-wb-hair px-2 py-0.5 text-[11px] text-wb-ink-mute"
				>
					<PaperclipIcon class="size-3 shrink-0" aria-hidden="true" />
					<span class="min-w-0 truncate">{name}</span>
				</li>
			{/each}
		</ul>
	{/if}
{/snippet}

<style>
	/* WebKit ignores list-style on summary; the chevron replaces the marker. */
	.steps-summary::-webkit-details-marker {
		display: none;
	}
	.skeleton {
		background: linear-gradient(
				90deg,
				var(--wb-panel) 0%,
				var(--wb-panel2) 50%,
				var(--wb-panel) 100%
			)
			0 0 / 200% 100%;
		animation: shimmer 1.6s ease-in-out infinite;
	}
	@keyframes shimmer {
		from {
			background-position: 100% 0;
		}
		to {
			background-position: -100% 0;
		}
	}

	/* Your message, sent but not yet picked up by the agent. */
	.pending {
		opacity: 0.6;
		animation: settle 220ms ease-out;
	}
	@keyframes settle {
		from {
			opacity: 0;
			transform: translateY(4px);
		}
	}

	.caret {
		display: inline-block;
		width: 7px;
		height: 14px;
		border-radius: 1px;
		background: var(--wb-agent, var(--wb-claude));
		animation: blink 1s steps(2, start) infinite;
	}
	@keyframes blink {
		to {
			visibility: hidden;
		}
	}

	.chat-btn {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		border-radius: 6px;
		border: 1px solid var(--wb-hair);
		padding: 4px 12px;
		font-size: 12px;
		color: var(--wb-ink);
	}
	.chat-btn:hover {
		border-color: var(--wb-ink-soft);
	}
	.chat-btn.primary {
		border-color: transparent;
		background: var(--wb-accent);
		color: var(--wb-accent-ink);
		font-weight: 600;
	}
	.chat-btn:focus-visible {
		outline: 2px solid var(--wb-accent);
		outline-offset: 1px;
	}

	@media (prefers-reduced-motion: reduce) {
		.skeleton,
		.pending,
		.caret {
			animation: none;
		}
	}
</style>
