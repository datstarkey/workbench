<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import { watch } from 'runed';
	import { OverlayScrollbars } from 'overlayscrollbars';
	import { overlayScrollbars } from '$lib/utils/overlay-scrollbars';
	import BotIcon from '@lucide/svelte/icons/bot';
	import RotateCwIcon from '@lucide/svelte/icons/rotate-cw';
	import { cn } from '@workbench/ui';
	import type { ChatImage, ProjectConfig } from '$types/workbench';
	import { getClaudeSessionStore } from '$stores/context';
	import { acquireChat } from './chat-registry';
	import ChatActivity from './ChatActivity.svelte';
	import ChatApproval from './ChatApproval.svelte';
	import ChatComposer from './ChatComposer.svelte';
	import ChatModelPicker from './ChatModelPicker.svelte';
	import ChatPlan from './ChatPlan.svelte';
	import ChatQuestion from './ChatQuestion.svelte';
	import ChatTasks from './ChatTasks.svelte';
	import ChatToolCard from './ChatToolCard.svelte';
	import {
		activity,
		formatTokens,
		groupBlocks,
		isRunning,
		latestTodos,
		limitNotice,
		splitFences,
		toolDetail
	} from './chat-format';

	let {
		paneId,
		sessionId,
		project,
		cwd,
		onShowTerminal,
		onSessionIdChange
	}: {
		paneId: string;
		/** Fixed for this component's life — the parent re-keys on a new session. */
		sessionId: string;
		project: ProjectConfig;
		cwd?: string;
		onShowTerminal: () => void;
		/** `/clear` moved the conversation to a new session id. */
		onSessionIdChange: (sessionId: string) => void;
	} = $props();

	const STARTERS = [
		'Explain how this project is structured',
		'Run the tests and fix what fails',
		'Review my uncommitted changes'
	];

	const workdir = $derived(cwd ?? project.path);
	const claudeSessionStore = getClaudeSessionStore();
	// svelte-ignore state_referenced_locally
	const { chat } = acquireChat(paneId, {
		projectPath: project.path,
		...(cwd && cwd !== project.path ? { worktreePath: cwd } : {}),
		sessionId,
		paneId
	});
	chat.onNeedsYou = (waiting) => claudeSessionStore.setAwaitingInput(paneId, waiting);

	watch(
		() => chat.sessionId,
		(id) => {
			if (id && id !== sessionId) onSessionIdChange(id);
		}
	);

	let draft = $state('');
	let stickToBottom = true;
	/** The tasks panel as an overlay, for panes too narrow to dock it. */
	let tasksOpen = $state(false);

	const blocks = $derived(groupBlocks(chat.items));
	const limit = $derived(
		limitNotice(chat.meta?.rateLimit ?? null, (secs) =>
			new Date(secs * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
		)
	);
	const tasks = $derived(chat.meta?.tasks ?? []);
	const runningTasks = $derived(tasks.filter(isRunning).length);
	const todos = $derived(latestTodos(chat.items));
	const now = $derived(activity(chat.items, chat.meta));
	const live = $derived(chat.status === 'live');
	const lastId = $derived(chat.items[chat.items.length - 1]?.id);
	const contextLimit = $derived(chat.meta?.model?.includes('[1m]') ? 1_000_000 : 200_000);
	const contextShare = $derived(
		chat.meta?.contextTokens ? Math.min(1, chat.meta.contextTokens / contextLimit) : 0
	);
	const disabledReason = $derived.by(() => {
		switch (chat.status) {
			case 'starting':
				return 'Starting Claude…';
			case 'reconnecting':
				return 'Reconnecting…';
			case 'exited':
			case 'failed':
				return 'Restart the session to send messages';
			default:
				return now.kind === 'approval' ? 'Answer Claude above first' : null;
		}
	});

	/**
	 * Keep the newest message in view, unless the reader has scrolled up.
	 * Attached after `overlayScrollbars()`, so it scrolls the viewport that
	 * library generates rather than the host element.
	 */
	const followLatest: Attachment<HTMLDivElement> = (host) => {
		const node = OverlayScrollbars(host)?.elements().viewport ?? host;
		const onScroll = () => {
			stickToBottom = node.scrollHeight - node.scrollTop - node.clientHeight < 48;
		};
		node.addEventListener('scroll', onScroll);
		watch(
			() => [chat.items, chat.pending, now.kind],
			() => {
				if (stickToBottom) node.scrollTop = node.scrollHeight;
			}
		);
		return () => node.removeEventListener('scroll', onScroll);
	};

	function send(text: string, images: ChatImage[]): boolean {
		stickToBottom = true;
		return chat.prompt(text, images);
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape' && chat.meta?.busy && now.kind !== 'approval') {
			event.preventDefault();
			chat.interrupt();
		}
	}
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="@container relative flex h-full min-h-0 flex-col bg-wb-bg text-sm text-wb-ink"
	onkeydown={onKeydown}
>
	<header class="flex h-9 shrink-0 items-center gap-3 border-b border-wb-hair pr-40 pl-4 text-xs">
		<span
			class={cn(
				'size-1.5 shrink-0 rounded-full',
				live && 'bg-wb-claude',
				(chat.status === 'starting' || chat.status === 'reconnecting') &&
					'animate-pulse bg-wb-warn',
				(chat.status === 'exited' || chat.status === 'failed') && 'bg-wb-ink-soft'
			)}
		></span>
		<span class="min-w-0 truncate font-medium">{chat.meta?.title ?? 'Claude'}</span>
		{#if chat.meta?.model}
			<span class="shrink-0 text-wb-ink-soft">{chat.meta.model.replace(/\[1m\]$/, '')}</span>
		{/if}
		{#if contextShare > 0}
			<span
				class="ml-auto flex shrink-0 items-center gap-1.5 text-wb-ink-soft tabular-nums"
				title="{formatTokens(chat.meta?.contextTokens ?? null)} tokens of context in use"
			>
				<span class="h-1 w-12 overflow-hidden rounded-full bg-wb-panel2">
					<span
						class={cn(
							'block h-full rounded-full',
							contextShare > 0.8 ? 'bg-wb-warn' : 'bg-wb-ink-soft'
						)}
						style:width="{contextShare * 100}%"
					></span>
				</span>
				{Math.round(contextShare * 100)}%
			</span>
		{/if}
		{#if tasks.length > 0}
			<button
				type="button"
				class={cn(
					'flex shrink-0 items-center gap-1.5 rounded-md px-2 py-0.5 hover:bg-wb-panel2 focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none @5xl:hidden',
					contextShare > 0 ? '' : 'ml-auto',
					runningTasks > 0 ? 'text-wb-claude' : 'text-wb-ink-mute'
				)}
				aria-expanded={tasksOpen}
				onclick={() => (tasksOpen = !tasksOpen)}
			>
				<BotIcon class="size-3.5" />
				{runningTasks > 0 ? `${runningTasks} running` : 'Tasks'}
			</button>
		{/if}
	</header>

	<div class="flex min-h-0 flex-1">
		<div class="flex min-h-0 min-w-0 flex-1 flex-col">
			{#if chat.status === 'reconnecting'}
				<div class="reconnect relative h-0.5 shrink-0 overflow-hidden bg-wb-panel2" role="status">
					<span class="sr-only">Reconnecting to Claude</span>
				</div>
			{/if}

			<div
				{@attach overlayScrollbars()}
				{@attach followLatest}
				class="min-h-0 flex-1"
				role="log"
				aria-live="polite"
			>
				<div class="mx-auto flex max-w-3xl flex-col gap-3.5 px-5 py-5">
					{#if chat.status === 'failed'}
						<div class="mx-auto mt-10 flex max-w-md flex-col items-center gap-3 text-center">
							<p class="font-medium">Claude couldn't start</p>
							<p class="text-xs text-wb-ink-mute">{chat.error}</p>
							<div class="flex gap-2">
								<button type="button" class="chat-btn primary" onclick={() => chat.open()}>
									Try again
								</button>
								<button type="button" class="chat-btn" onclick={onShowTerminal}
									>Use the terminal</button
								>
							</div>
						</div>
					{:else if chat.status === 'starting' && chat.items.length === 0}
						<div class="flex flex-col gap-3 pt-2" aria-label="Starting Claude">
							<span class="skeleton h-9 w-2/5 self-end rounded-lg"></span>
							<span class="skeleton h-3 w-4/5 rounded"></span>
							<span class="skeleton h-3 w-3/5 rounded"></span>
							<span class="skeleton h-8 w-full rounded-md"></span>
							<p class="pt-1 text-xs text-wb-ink-soft">Starting Claude in {project.name}…</p>
						</div>
					{:else if chat.items.length === 0 && chat.pending.length === 0}
						<div class="mt-[12vh] flex flex-col gap-4">
							<h2 class="text-lg font-medium text-balance">What should Claude work on?</h2>
							<p class="text-xs text-wb-ink-mute">
								Working in <span class="font-mono text-wb-ink">{workdir}</span>
							</p>
							<div class="flex flex-wrap gap-2">
								{#each STARTERS as starter (starter)}
									<button
										type="button"
										class="rounded-full border border-wb-hair px-3 py-1.5 text-xs text-wb-ink-mute hover:border-wb-ink-soft hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
										onclick={() => (draft = starter)}
									>
										{starter}
									</button>
								{/each}
							</div>
						</div>
					{/if}

					{#if chat.start > 0}
						<p class="text-center text-xs text-wb-ink-soft">
							Earlier messages are in the
							<button type="button" class="underline hover:text-wb-ink" onclick={onShowTerminal}
								>terminal</button
							>.
						</p>
					{/if}

					{#each blocks as block (block.kind === 'item' ? block.item.id : block.id)}
						{#if block.kind === 'quiet'}
							<div class="flex flex-wrap gap-1.5">
								{#each block.tools as tool (tool.id)}
									<span
										class="max-w-full truncate rounded-md border border-wb-hair px-1.5 py-0.5 font-mono text-[11px] text-wb-ink-mute"
										title={toolDetail(tool, workdir)}
									>
										{tool.name} <span class="text-wb-ink">{toolDetail(tool, workdir)}</span>
									</span>
								{/each}
							</div>
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
									<span
										class="rounded-md border border-wb-hair px-2 py-0.5 text-[11px] text-wb-ink-mute"
									>
										{block.item.images === 1 ? '1 image' : `${block.item.images} images`}
									</span>
								{/if}
								{#if block.item.text}
									<div
										class="rounded-2xl rounded-br-md bg-wb-panel2 px-3.5 py-2 whitespace-pre-wrap"
									>
										{block.item.text}
									</div>
								{/if}
							</div>
						{:else if block.item.kind === 'text'}
							<div class="flex min-w-0 flex-col gap-2 leading-relaxed">
								{#each splitFences(block.item.text) as segment, i (i)}
									{#if segment.kind === 'code'}
										<pre
											class="scrollbar-thin overflow-x-auto rounded-md border border-wb-hair bg-wb-panel px-3 py-2 font-mono text-xs">{segment.text}</pre>
									{:else}
										<p class="whitespace-pre-wrap">{segment.text}</p>
									{/if}
								{/each}
								{#if now.kind === 'writing' && block.item.id === lastId}
									<span class="caret" aria-hidden="true"></span>
								{/if}
							</div>
						{:else if block.item.kind === 'thinking'}
							{#if block.item.text}
								<details class="text-xs text-wb-ink-soft">
									<summary class="w-fit cursor-pointer select-none hover:text-wb-ink-mute">
										Thinking
									</summary>
									<p class="mt-1.5 border-l border-wb-hair pl-3 whitespace-pre-wrap">
										{block.item.text}
									</p>
								</details>
							{/if}
						{:else if block.item.kind === 'tool'}
							<ChatToolCard
								tool={block.item}
								cwd={workdir}
								startedAt={chat.seenAt[block.item.id]}
								fetchFullOutput={(id) => chat.fullOutput(id)}
							/>
						{:else if block.item.kind === 'approval' && block.item.tool === 'AskUserQuestion'}
							{@const approval = block.item}
							<ChatQuestion
								{approval}
								onAnswer={(decision, answers) => chat.approve(approval.id, decision, answers)}
							/>
						{:else if block.item.kind === 'approval'}
							{@const approval = block.item}
							<ChatApproval
								{approval}
								cwd={workdir}
								onDecide={(decision) => chat.approve(approval.id, decision)}
							/>
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
							{#if prompt.text}
								<div class="rounded-2xl rounded-br-md bg-wb-panel2 px-3.5 py-2 whitespace-pre-wrap">
									{prompt.text}
								</div>
							{/if}
						</div>
					{/each}

					{#if now.kind !== 'idle' && live}
						<ChatActivity
							activity={now}
							since={chat.busySince}
							cwd={workdir}
							onStop={() => chat.interrupt()}
						/>
					{/if}

					{#if chat.status === 'exited'}
						<div
							class="flex items-center gap-3 rounded-lg border border-wb-hair px-3.5 py-2.5 text-xs"
						>
							<span class="flex-1 text-wb-ink-mute">
								{chat.error ?? 'Claude stopped. The conversation is saved.'}
							</span>
							<button type="button" class="chat-btn" onclick={() => chat.open()}>
								<RotateCwIcon class="size-3" /> Restart
							</button>
						</div>
					{/if}
				</div>
			</div>

			<div class="mx-auto flex w-full max-w-3xl shrink-0 flex-col gap-2 px-5 pb-4">
				{#if chat.notice}
					<p class="text-xs text-wb-err" role="alert">{chat.notice}</p>
				{/if}
				{#if limit}
					<p
						class={cn(
							'rounded-md border px-3 py-2 text-xs',
							limit.tone === 'blocked'
								? 'border-wb-warn/50 bg-wb-warn/10 text-wb-ink'
								: 'border-wb-hair text-wb-ink-mute'
						)}
						role={limit.tone === 'blocked' ? 'alert' : undefined}
					>
						{limit.text}
					</p>
				{/if}
				{#if todos.length > 0}
					<ChatPlan steps={todos} />
				{/if}
				<ChatComposer
					id="chat-draft-{paneId}"
					bind:draft
					mode={chat.meta?.permissionMode ?? null}
					busy={Boolean(chat.meta?.busy) && live}
					{disabledReason}
					onSend={send}
					onStop={() => chat.interrupt()}
					onMode={(mode) => chat.setMode(mode)}
				>
					{#snippet controls()}
						<ChatModelPicker
							meta={chat.meta}
							disabled={disabledReason !== null}
							onModel={(model) => chat.setModel(model)}
							onEffort={(effort) => chat.setEffort(effort)}
						/>
					{/snippet}
				</ChatComposer>
			</div>
		</div>
		{#if tasks.length > 0}
			<div class="hidden @5xl:flex">
				<ChatTasks {tasks} seenAt={chat.seenAt} fetchOutput={(id) => chat.taskOutput(id)} />
			</div>
		{/if}
	</div>

	{#if tasksOpen && tasks.length > 0}
		<div class="absolute inset-y-0 right-0 z-20 flex shadow-2xl @5xl:hidden">
			<ChatTasks
				{tasks}
				seenAt={chat.seenAt}
				fetchOutput={(id) => chat.taskOutput(id)}
				onClose={() => (tasksOpen = false)}
			/>
		</div>
	{/if}
</div>

<style>
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

	/* Your message, sent but not yet picked up by Claude. */
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
		background: var(--wb-claude);
		animation: blink 1s steps(2, start) infinite;
	}
	@keyframes blink {
		to {
			visibility: hidden;
		}
	}

	.reconnect::after {
		content: '';
		position: absolute;
		inset: 0;
		width: 30%;
		background: var(--wb-warn);
		animation: travel 1.2s ease-in-out infinite;
	}
	@keyframes travel {
		from {
			transform: translateX(-100%);
		}
		to {
			transform: translateX(340%);
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
		.caret,
		.reconnect::after {
			animation: none;
		}
	}
</style>
