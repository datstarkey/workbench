<script lang="ts">
	import { onMount } from 'svelte';
	import BotIcon from '@lucide/svelte/icons/bot';
	import CheckIcon from '@lucide/svelte/icons/check';
	import TerminalSquareIcon from '@lucide/svelte/icons/square-terminal';
	import XIcon from '@lucide/svelte/icons/x';
	import { cn } from '@workbench/ui';
	import type { TaskInfo } from '$types/workbench';
	import { formatCount, formatElapsed, isRunning, sortTasks } from './chat-format';

	let {
		tasks,
		seenAt,
		onClose
	}: {
		tasks: TaskInfo[];
		/** When each task was first seen, for running timers. */
		seenAt: Record<string, number>;
		/** Set when shown as an overlay (narrow pane). */
		onClose?: () => void;
	} = $props();

	let now = $state(Date.now());
	onMount(() => {
		const timer = setInterval(() => (now = Date.now()), 1000);
		return () => clearInterval(timer);
	});

	const groups = $derived(sortTasks(tasks));
	const running = $derived(tasks.filter(isRunning).length);

	function elapsed(task: TaskInfo): string {
		if (isRunning(task) && seenAt[task.id]) return formatElapsed(now - seenAt[task.id]);
		return task.durationMs > 0 ? formatElapsed(task.durationMs) : '';
	}

	function title(task: TaskInfo): string {
		if (task.kind === 'agent') return task.subagentType ?? 'Agent';
		return task.kind === 'local_bash' ? 'Shell' : task.kind.replace(/^local_/, '');
	}
</script>

{#snippet card(task: TaskInfo)}
	{@const live = isRunning(task)}
	<li
		class={cn(
			'relative overflow-hidden rounded-lg border px-3 py-2.5',
			live ? 'border-wb-hair bg-wb-panel2' : 'border-transparent'
		)}
	>
		<div class="flex items-center gap-2 text-xs">
			{#if live}
				<span class="spinner size-3 shrink-0" aria-label="Running"></span>
			{:else if task.status === 'completed'}
				<CheckIcon class="size-3.5 shrink-0 text-wb-ok" aria-label="Done" />
			{:else}
				<XIcon class="size-3.5 shrink-0 text-wb-err" aria-label={task.status} />
			{/if}
			<span class={cn('min-w-0 truncate font-medium', live ? 'text-wb-ink' : 'text-wb-ink-mute')}>
				{title(task)}
			</span>
			{#if task.background}
				<span class="shrink-0 text-[10px] text-wb-ink-soft">background</span>
			{/if}
			<span class="ml-auto shrink-0 text-[11px] text-wb-ink-soft tabular-nums">{elapsed(task)}</span
			>
		</div>
		<p class={cn('mt-1 text-xs leading-snug', live ? 'text-wb-ink' : 'text-wb-ink-mute')}>
			{task.description}
		</p>
		{#if live && (task.activity || task.lastTool)}
			<p class="mt-1 truncate text-[11px] text-wb-ink-mute">
				{task.activity ?? `Using ${task.lastTool}`}
			</p>
		{:else if !live && task.summary}
			<p class="mt-1 line-clamp-3 text-[11px] leading-snug text-wb-ink-soft">{task.summary}</p>
		{/if}
		{#if task.toolUses > 0 || task.tokens > 0}
			<p class="mt-1.5 text-[10px] text-wb-ink-soft tabular-nums">
				{formatCount(task.toolUses, task.toolUses === 1 ? 'tool call' : 'tool calls')}
				{#if task.tokens > 0}<span class="px-1">/</span>{formatCount(task.tokens, 'tokens')}{/if}
			</p>
		{/if}
		{#if live}
			<span class="sweep" aria-hidden="true"></span>
		{/if}
	</li>
{/snippet}

{#snippet group(label: string, list: TaskInfo[])}
	{#if list.length > 0}
		<section class="flex flex-col gap-1.5">
			<h3 class="flex items-center gap-1.5 px-1 text-[11px] text-wb-ink-soft">
				{#if label === 'Agents'}<BotIcon class="size-3" />{:else}<TerminalSquareIcon
						class="size-3"
					/>{/if}
				{label}
				<span class="tabular-nums">{list.length}</span>
			</h3>
			<ul class="flex flex-col gap-1">
				{#each list as task (task.id)}
					{@render card(task)}
				{/each}
			</ul>
		</section>
	{/if}
{/snippet}

<aside class="flex h-full min-h-0 w-72 flex-col border-l border-wb-hair bg-wb-panel">
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
	<div class="scrollbar-thin flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-2.5">
		{#if tasks.length === 0}
			<p class="px-1 pt-2 text-xs leading-relaxed text-wb-ink-soft">
				Subagents and background commands Claude starts appear here while they run.
			</p>
		{/if}
		{@render group('Agents', groups.agents)}
		{@render group('Background', groups.jobs)}
	</div>
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
	.sweep {
		position: absolute;
		inset: auto 0 0 0;
		height: 1px;
		background: linear-gradient(90deg, transparent 0%, var(--wb-claude) 50%, transparent 100%) 0 0 /
			40% 100% no-repeat;
		animation: sweep 1.6s ease-in-out infinite;
	}
	@keyframes sweep {
		from {
			background-position: -40% 0;
		}
		to {
			background-position: 140% 0;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.spinner,
		.sweep {
			animation: none;
		}
	}
</style>
