<script lang="ts">
	import { onDestroy } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import SquareIcon from '@lucide/svelte/icons/square';
	import { cn } from '@workbench/ui';
	import { getClaudeSessionStore } from '$stores/context';
	import ChatToolCard from './ChatToolCard.svelte';
	import { formatTokens, groupBlocks, latestTodos, splitFences, toolDetail } from './chat-format';
	import { paneInput, submitPrompt } from './pane-input';
	import { TranscriptFeed } from './transcript-feed.svelte';

	let {
		paneId,
		sessionId,
		cwd,
		onShowTerminal
	}: {
		paneId: string;
		/** Fixed for this component's life — the parent re-keys on a new session. */
		sessionId: string;
		cwd?: string;
		onShowTerminal: () => void;
	} = $props();

	const claudeSessionStore = getClaudeSessionStore();
	// svelte-ignore state_referenced_locally
	const feed = new TranscriptFeed(sessionId);
	onDestroy(() => feed.dispose());

	let draft = $state('');
	let sendError = $state('');
	let stickToBottom = true;

	const blocks = $derived(groupBlocks(feed.items));
	const todos = $derived(latestTodos(feed.items));
	const doneCount = $derived(todos.filter((t) => t.status === 'completed').length);
	const busy = $derived(feed.meta?.busy ?? false);
	const awaitingAnswer = $derived(claudeSessionStore.panesAwaitingInput.has(paneId));

	/** Keep the newest message in view, unless the reader has scrolled up. */
	const followLatest: Attachment<HTMLDivElement> = (node) => {
		const onScroll = () => {
			stickToBottom = node.scrollHeight - node.scrollTop - node.clientHeight < 48;
		};
		node.addEventListener('scroll', onScroll);
		$effect(() => {
			void feed.items;
			if (stickToBottom) node.scrollTop = node.scrollHeight;
		});
		return () => node.removeEventListener('scroll', onScroll);
	};

	function send() {
		const text = draft.trim();
		if (!text) return;
		if (!submitPrompt(paneId, text)) {
			sendError = 'The terminal for this session is not connected.';
			return;
		}
		sendError = '';
		draft = '';
		stickToBottom = true;
	}

	function onComposerKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
			event.preventDefault();
			send();
		}
	}

	/** Esc interrupts the running turn, as in the terminal. */
	function interrupt() {
		paneInput(paneId)?.key('\x1b');
	}

	/** Shift+Tab cycles the permission mode, as in the terminal. */
	function cycleMode() {
		paneInput(paneId)?.key('\x1b[Z');
	}
</script>

<div class="flex h-full min-h-0 flex-col bg-wb-bg text-sm text-wb-ink">
	<div class="flex h-8 shrink-0 items-center gap-3 border-b border-wb-hair px-3 pr-28 text-xs">
		<span
			class={cn(
				'size-1.5 shrink-0 rounded-full',
				feed.status === 'live' ? 'bg-wb-claude' : 'animate-pulse bg-wb-ink-soft'
			)}
			title={feed.status === 'live' ? 'Live' : 'Connecting…'}
		></span>
		<span class="min-w-0 truncate font-medium">{feed.meta?.title ?? 'Claude session'}</span>
		{#if feed.meta?.model}
			<span class="shrink-0 font-mono text-[11px] text-wb-ink-soft">{feed.meta.model}</span>
		{/if}
		{#if feed.meta?.contextTokens}
			<span class="shrink-0 font-mono text-[11px] text-wb-ink-soft tabular-nums">
				{formatTokens(feed.meta.contextTokens)} context
			</span>
		{/if}
	</div>

	<div
		{@attach followLatest}
		class="min-h-0 flex-1 overflow-y-auto px-4 py-4"
		role="log"
		aria-live="polite"
	>
		<div class="mx-auto flex max-w-3xl flex-col gap-3">
			{#if feed.truncated}
				<p class="text-center text-xs text-wb-ink-soft">
					Earlier messages are in the
					<button type="button" class="underline hover:text-wb-ink" onclick={onShowTerminal}>
						terminal
					</button>.
				</p>
			{/if}
			{#if feed.items.length === 0}
				<p class="py-10 text-center text-xs text-wb-ink-soft">
					{feed.status === 'live'
						? 'Messages appear here as the session runs.'
						: 'Connecting to the session…'}
				</p>
			{/if}

			{#each blocks as block (block.kind === 'item' ? block.item.id : block.id)}
				{#if block.kind === 'quiet'}
					<div class="flex flex-wrap gap-1.5">
						{#each block.tools as tool (tool.id)}
							<span
								class="max-w-full truncate rounded border border-wb-hair bg-wb-panel px-1.5 font-mono text-[11px] text-wb-ink-mute"
								title={toolDetail(tool, cwd)}
							>
								{tool.name} <span class="text-wb-ink">{toolDetail(tool, cwd)}</span>
							</span>
						{/each}
					</div>
				{:else if block.item.kind === 'user'}
					<div
						class="max-w-[85%] self-end rounded-lg rounded-br-sm border border-wb-hair bg-wb-panel2 px-3 py-2 whitespace-pre-wrap"
					>
						{block.item.text}
					</div>
				{:else if block.item.kind === 'text'}
					<div class="flex min-w-0 flex-col gap-2 leading-relaxed">
						{#each splitFences(block.item.text) as segment, i (i)}
							{#if segment.kind === 'code'}
								<pre
									class="overflow-x-auto rounded-md border border-wb-hair bg-wb-panel px-3 py-2 font-mono text-xs">{segment.text}</pre>
							{:else}
								<p class="whitespace-pre-wrap">{segment.text}</p>
							{/if}
						{/each}
					</div>
				{:else if block.item.kind === 'thinking'}
					<details class="text-xs text-wb-ink-soft">
						<summary class="cursor-pointer select-none">Thinking</summary>
						<p class="mt-1 whitespace-pre-wrap">{block.item.text}</p>
					</details>
				{:else if block.item.kind === 'tool'}
					<ChatToolCard tool={block.item} {cwd} />
				{:else}
					<p class="text-center text-xs text-wb-ink-soft">{block.item.text}</p>
				{/if}
			{/each}
		</div>
	</div>

	{#if todos.length > 0}
		<details class="mx-4 mb-2 shrink-0 rounded-md border border-wb-hair bg-wb-panel text-xs">
			<summary class="cursor-pointer px-3 py-1.5 text-wb-ink-mute select-none">
				Plan · {doneCount}/{todos.length} done
			</summary>
			<ul class="flex flex-col gap-0.5 px-3 pb-2">
				{#each todos as todo, i (i)}
					<li
						class={cn(
							todo.status === 'completed' && 'text-wb-ink-soft line-through',
							todo.status === 'in_progress' && 'text-wb-ink',
							todo.status === 'pending' && 'text-wb-ink-mute'
						)}
					>
						{todo.content}
					</li>
				{/each}
			</ul>
		</details>
	{/if}

	{#if awaitingAnswer}
		<div
			class="mx-4 mb-2 flex shrink-0 items-center gap-3 rounded-md border border-wb-warn/50 bg-wb-warn/10 px-3 py-2 text-xs"
		>
			<span class="flex-1">Claude is asking a question in the terminal.</span>
			<button
				type="button"
				class="rounded bg-wb-warn px-2 py-0.5 font-medium text-wb-accent-ink"
				onclick={onShowTerminal}
			>
				Answer in terminal
			</button>
		</div>
	{/if}

	<div class="mx-4 mb-3 shrink-0 rounded-lg border border-wb-hair bg-wb-panel">
		<label for="chat-draft-{paneId}" class="sr-only">Message Claude</label>
		<textarea
			id="chat-draft-{paneId}"
			bind:value={draft}
			onkeydown={onComposerKeydown}
			rows="2"
			placeholder="Message Claude. Enter to send, Shift+Enter for a new line."
			class="block w-full resize-none bg-transparent px-3 pt-2 text-sm placeholder:text-wb-ink-soft focus:outline-none"
		></textarea>
		<div class="flex items-center gap-2 px-2 pb-2 text-[11px] text-wb-ink-mute">
			<button
				type="button"
				class="rounded border border-wb-hair px-1.5 py-0.5 hover:text-wb-ink"
				title="Cycle permission mode (Shift+Tab)"
				onclick={cycleMode}
			>
				{feed.meta?.permissionMode ?? 'mode'}
			</button>
			{#if sendError}
				<span class="text-wb-err">{sendError}</span>
			{/if}
			<span class="flex-1"></span>
			{#if busy}
				<button
					type="button"
					class="flex size-7 items-center justify-center rounded-md border border-wb-hair bg-wb-panel2 hover:text-wb-ink"
					title="Interrupt (Esc)"
					aria-label="Interrupt"
					onclick={interrupt}
				>
					<SquareIcon class="size-3 fill-current" />
				</button>
			{/if}
			<button
				type="button"
				class="flex size-7 items-center justify-center rounded-md bg-wb-accent text-wb-accent-ink disabled:opacity-40"
				aria-label="Send"
				disabled={!draft.trim()}
				onclick={send}
			>
				<ArrowUpIcon class="size-3.5" />
			</button>
		</div>
	</div>
</div>
