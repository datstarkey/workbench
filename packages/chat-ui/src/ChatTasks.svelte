<script lang="ts">
	import { onMount } from 'svelte';
	import BotIcon from '@lucide/svelte/icons/bot';
	import CheckIcon from '@lucide/svelte/icons/check';
	import TerminalSquareIcon from '@lucide/svelte/icons/square-terminal';
	import XIcon from '@lucide/svelte/icons/x';
	import { cn } from '@workbench/ui';
	import type { TaskInfo } from '@workbench/types';
	import type { TaskOutput } from './agent-chat.svelte';
	import { formatCount, formatElapsed, isRunning, sortTasks } from './chat-format';
	import ChatTaskOutput from './ChatTaskOutput.svelte';

	let {
		tasks,
		seenAt,
		fetchOutput,
		onClose,
		class: className
	}: {
		tasks: TaskInfo[];
		/** When each task was first seen, for running timers. */
		seenAt: Record<string, number>;
		fetchOutput: (taskId: string) => Promise<TaskOutput | null>;
		/** Set when shown as an overlay (narrow pane). */
		onClose?: () => void;
		class?: string;
	} = $props();

	let now = $state(Date.now());
	onMount(() => {
		const timer = setInterval(() => (now = Date.now()), 1000);
		return () => clearInterval(timer);
	});

	const groups = $derived(sortTasks(tasks));
	const running = $derived(tasks.filter(isRunning).length);

	/** Agents and background jobs are separate tabs; until one is picked, agents if any. */
	let pickedTab = $state<'agents' | 'jobs' | null>(null);
	const tab = $derived(
		pickedTab && groups[pickedTab].length > 0
			? pickedTab
			: groups.agents.length > 0
				? 'agents'
				: 'jobs'
	);
	const list = $derived(groups[tab]);

	/** The task whose detail and output are shown; the first (running first) until one is picked. */
	let pickedId = $state<string | null>(null);
	const selected = $derived(list.find((t) => t.id === pickedId) ?? list[0] ?? null);

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

{#snippet tabButton(key: 'agents' | 'jobs', label: string, count: number)}
	<button
		type="button"
		aria-pressed={tab === key}
		disabled={count === 0}
		class={cn(
			'flex flex-1 items-center justify-center gap-1.5 rounded-md px-2 py-1 text-[11px] focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none disabled:opacity-40',
			tab === key ? 'bg-wb-panel2 text-wb-ink' : 'text-wb-ink-mute hover:text-wb-ink'
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
		<div class="flex shrink-0 gap-1 border-b border-wb-hair p-1.5" role="group" aria-label="Show">
			{@render tabButton('agents', 'Agents', groups.agents.length)}
			{@render tabButton('jobs', 'Background', groups.jobs.length)}
		</div>
		<ul
			class="scrollbar-thin flex max-h-48 shrink-0 flex-col gap-0.5 overflow-y-auto border-b border-wb-hair p-1.5"
			aria-label={tab === 'agents' ? 'Agents' : 'Background tasks'}
		>
			{#each list as task (task.id)}
				<li>
					<button
						type="button"
						aria-current={selected?.id === task.id}
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
						<span class="shrink-0 text-[10.5px] text-wb-ink-soft tabular-nums">{elapsed(task)}</span
						>
					</button>
				</li>
			{/each}
		</ul>
		{#if selected}
			{@const live = isRunning(selected)}
			<section
				class="scrollbar-thin flex min-h-0 flex-1 flex-col overflow-y-auto px-3 py-2.5"
				aria-label="{title(selected)} details"
			>
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
				{#key selected.id}
					<ChatTaskOutput taskId={selected.id} {live} {fetchOutput} />
				{/key}
			</section>
		{/if}
	{/if}
</aside>

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
