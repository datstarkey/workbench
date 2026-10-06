<script lang="ts">
	import { onMount } from 'svelte';
	import BotIcon from '@lucide/svelte/icons/bot';
	import CheckIcon from '@lucide/svelte/icons/check';
	import TerminalSquareIcon from '@lucide/svelte/icons/square-terminal';
	import XIcon from '@lucide/svelte/icons/x';
	import { cn } from '@workbench/ui';
	import type { TaskInfo, TaskTranscript } from '@workbench/types';
	import type { TaskOutput } from './agent-chat.svelte';
	import {
		formatCount,
		formatElapsed,
		isRunning,
		pickTasks,
		pickTaskView,
		taskViews,
		type TaskTab,
		type TaskView
	} from './chat-format';
	import ChatTaskOutput from './ChatTaskOutput.svelte';
	import ChatTaskTranscript from './ChatTaskTranscript.svelte';

	let {
		tasks,
		seenAt,
		cwd,
		fetchOutput,
		fetchTranscript,
		onClose,
		class: className
	}: {
		tasks: TaskInfo[];
		/** When each task was first seen, for running timers. */
		seenAt: Record<string, number>;
		/** The chat's cwd, so tool cards show paths relative to it. */
		cwd?: string;
		fetchOutput: (taskId: string) => Promise<TaskOutput | null>;
		fetchTranscript: (taskId: string) => Promise<TaskTranscript | null>;
		/** Set when shown as an overlay (narrow pane). */
		onClose?: () => void;
		class?: string;
	} = $props();

	let now = $state(Date.now());
	onMount(() => {
		const timer = setInterval(() => (now = Date.now()), 1000);
		return () => clearInterval(timer);
	});

	const uid = $props.id();
	const running = $derived(tasks.filter(isRunning).length);
	let pickedTab = $state<TaskTab | null>(null);
	let pickedId = $state<string | null>(null);
	const view = $derived(pickTasks(tasks, pickedTab, pickedId));
	const list = $derived(view[view.tab]);
	const selected = $derived(view.selected);
	let pickedView = $state<TaskView | null>(null);
	const detailView = $derived(selected ? pickTaskView(selected, pickedView) : 'output');
	const VIEW_LABELS: Record<TaskView, string> = { conversation: 'Conversation', output: 'Output' };

	function elapsed(task: TaskInfo): string {
		if (isRunning(task) && seenAt[task.id]) return formatElapsed(now - seenAt[task.id]);
		return task.durationMs > 0 ? formatElapsed(task.durationMs) : '';
	}

	function title(task: TaskInfo): string {
		if (task.kind === 'agent') return task.subagentType ?? 'Agent';
		return task.kind === 'local_bash' ? 'Shell' : task.kind.replace(/^local_/, '');
	}
</script>

{#snippet status(task: TaskInfo)}
	{#if isRunning(task)}
		<span class="spinner size-3 shrink-0" aria-label="Running"></span>
	{:else if task.status === 'completed'}
		<CheckIcon class="size-3.5 shrink-0 text-wb-ok" aria-label="Done" />
	{:else}
		<XIcon class="size-3.5 shrink-0 text-wb-err" aria-label={task.status} />
	{/if}
{/snippet}

{#snippet tabButton(key: TaskTab, label: string, count: number)}
	<button
		type="button"
		role="tab"
		id="{uid}-{key}"
		aria-selected={view.tab === key}
		aria-controls="{uid}-panel"
		disabled={count === 0}
		class={cn(
			'flex flex-1 items-center justify-center gap-1.5 rounded-md px-2 py-1 text-[11px] focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none disabled:opacity-40',
			view.tab === key ? 'bg-wb-panel2 text-wb-ink' : 'text-wb-ink-mute hover:text-wb-ink'
		)}
		onclick={() => (pickedTab = key)}
	>
		{#if key === 'agents'}<BotIcon class="size-3" />{:else}<TerminalSquareIcon
				class="size-3"
			/>{/if}
		{label}
		<span class="text-wb-ink-soft tabular-nums">{count}</span>
	</button>
{/snippet}

<aside
	class={cn('flex h-full min-h-0 w-72 flex-col border-l border-wb-hair bg-wb-panel', className)}
>
	<header class="flex h-9 shrink-0 items-center gap-2 border-b border-wb-hair px-3 text-xs">
		<span class="font-medium text-wb-ink">Agents and tasks</span>
		{#if running > 0}
			<span class="rounded-full bg-wb-claude/15 px-1.5 text-[11px] text-wb-claude tabular-nums">
				{running} running
			</span>
		{/if}
		{#if onClose}
			<button
				type="button"
				class="ml-auto rounded p-0.5 text-wb-ink-soft hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
				aria-label="Close panel"
				onclick={onClose}
			>
				<XIcon class="size-3.5" />
			</button>
		{/if}
	</header>
	{#if tasks.length === 0}
		<p class="px-3.5 pt-3 text-xs leading-relaxed text-wb-ink-soft">
			Subagents and background commands Claude starts appear here while they run.
		</p>
	{:else}
		<div
			class="flex shrink-0 gap-1 border-b border-wb-hair p-1.5"
			role="tablist"
			aria-label="Agents and tasks"
		>
			{@render tabButton('agents', 'Agents', view.agents.length)}
			{@render tabButton('jobs', 'Background', view.jobs.length)}
		</div>
		<div
			id="{uid}-panel"
			role="tabpanel"
			aria-labelledby="{uid}-{view.tab}"
			class="flex min-h-0 flex-1 flex-col"
		>
			<ul
				class="scrollbar-thin flex max-h-48 shrink-0 flex-col gap-0.5 overflow-y-auto border-b border-wb-hair p-1.5"
			>
				{#each list as task (task.id)}
					<li>
						<button
							type="button"
							aria-pressed={selected?.id === task.id}
							aria-controls="{uid}-detail"
							class={cn(
								'flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none',
								selected?.id === task.id
									? 'bg-wb-panel2 text-wb-ink'
									: 'text-wb-ink-mute hover:bg-wb-panel2/60'
							)}
							onclick={() => (pickedId = task.id)}
						>
							{@render status(task)}
							<span class="min-w-0 flex-1 truncate">{task.description || title(task)}</span>
							<span class="shrink-0 text-[10.5px] text-wb-ink-soft tabular-nums"
								>{elapsed(task)}</span
							>
						</button>
					</li>
				{/each}
			</ul>
			{#if selected}
				{@const live = isRunning(selected)}
				{@const views = taskViews(selected)}
				<section
					id="{uid}-detail"
					class="flex min-h-0 flex-1 flex-col"
					aria-label="{title(selected)} details"
				>
					<div class="shrink-0 px-3 pt-2.5">
						{@render summary(selected, live)}
						{#if views.length > 1}
							<div
								class="mt-2 flex gap-0.5 rounded-md border border-wb-hair p-0.5"
								role="group"
								aria-label="Show"
							>
								{#each views as v (v)}
									<button
										type="button"
										aria-pressed={detailView === v}
										class={cn(
											'flex-1 rounded px-2 py-0.5 text-[11px] focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none',
											detailView === v
												? 'bg-wb-panel2 text-wb-ink'
												: 'text-wb-ink-mute hover:text-wb-ink'
										)}
										onclick={() => (pickedView = v)}
									>
										{VIEW_LABELS[v]}
									</button>
								{/each}
							</div>
						{/if}
					</div>
					{#key `${selected.id}:${detailView}`}
						{#if detailView === 'conversation'}
							<ChatTaskTranscript
								taskId={selected.id}
								{live}
								{cwd}
								{fetchTranscript}
								class="px-3 pt-2.5 pb-3"
							/>
						{:else}
							<div class="scrollbar-thin min-h-0 flex-1 overflow-y-auto px-3 pb-2.5">
								<ChatTaskOutput taskId={selected.id} {live} {fetchOutput} />
							</div>
						{/if}
					{/key}
				</section>
			{/if}
		</div>
	{/if}
</aside>

{#snippet summary(selected: TaskInfo, live: boolean)}
	<div class="flex items-center gap-2 text-xs">
		{@render status(selected)}
		<span class="min-w-0 truncate font-medium text-wb-ink">{title(selected)}</span>
		{#if selected.background}
			<span class="shrink-0 text-[10px] text-wb-ink-soft">background</span>
		{/if}
		<span class="ml-auto shrink-0 text-[11px] text-wb-ink-soft tabular-nums">
			{elapsed(selected)}
		</span>
	</div>
	<p class="mt-1 text-xs leading-snug text-wb-ink">{selected.description}</p>
	{#if live && (selected.activity || selected.lastTool)}
		<p class="mt-1 truncate text-[11px] text-wb-ink-mute">
			{selected.activity ?? `Using ${selected.lastTool}`}
		</p>
	{:else if !live && selected.summary}
		<p class="mt-1 text-[11px] leading-snug text-wb-ink-soft">{selected.summary}</p>
	{/if}
	{#if selected.toolUses > 0 || selected.tokens > 0}
		<p class="mt-1.5 text-[10px] text-wb-ink-soft tabular-nums">
			{formatCount(selected.toolUses, selected.toolUses === 1 ? 'tool call' : 'tool calls')}
			{#if selected.tokens > 0}<span class="px-1">/</span>{formatCount(
					selected.tokens,
					'tokens'
				)}{/if}
		</p>
	{/if}
{/snippet}

<style>
	.spinner {
		border-radius: 999px;
		border: 1.5px solid var(--wb-hair);
		border-top-color: var(--wb-claude);
		animation: spin 0.8s linear infinite;
	}
	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.spinner {
			animation: none;
		}
	}
</style>
