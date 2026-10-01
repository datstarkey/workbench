<script lang="ts">
	import { onMount } from 'svelte';
	import { cn } from '@workbench/ui';
	import { type Activity, formatElapsed, toolDetail } from './chat-format';

	let {
		activity,
		since,
		cwd,
		onStop
	}: {
		activity: Exclude<Activity, { kind: 'idle' }>;
		/** Turn start (ms), for the timer. */
		since: number | null;
		cwd?: string;
		onStop: () => void;
	} = $props();

	let now = $state(Date.now());
	onMount(() => {
		const timer = setInterval(() => (now = Date.now()), 1000);
		return () => clearInterval(timer);
	});

	const label = $derived.by(() => {
		switch (activity.kind) {
			case 'approval':
				if (activity.approval.tool === 'AskUserQuestion') return 'Waiting for your answer';
				if (activity.approval.tool === 'ExitPlanMode') return 'Waiting for you to review the plan';
				return 'Waiting for your approval';
			case 'tool':
				return `Running ${activity.tool.name}`;
			case 'writing':
				return 'Writing';
			case 'thinking':
				return 'Thinking';
		}
	});
	const detail = $derived(activity.kind === 'tool' ? toolDetail(activity.tool, cwd) : '');
	const waiting = $derived(activity.kind === 'approval');
</script>

<div
	class={cn('activity flex h-7 items-center gap-2.5 text-xs', waiting && 'waiting')}
	role="status"
	aria-live="polite"
>
	<span class="bars" aria-hidden="true"><i></i><i></i><i></i></span>
	<span class={cn('shrink-0 font-medium', waiting ? 'text-wb-warn' : 'label text-wb-ink')}
		>{label}</span
	>
	{#if detail}
		<span class="min-w-0 truncate font-mono text-[11px] text-wb-ink-mute">{detail}</span>
	{/if}
	{#if since !== null && !waiting}
		<span class="shrink-0 text-[11px] text-wb-ink-soft tabular-nums">
			{formatElapsed(now - since)}
		</span>
	{/if}
	<span class="flex-1"></span>
	{#if !waiting}
		<button
			type="button"
			class="flex shrink-0 items-center gap-1.5 rounded-md px-2 py-0.5 text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
			onclick={onStop}
		>
			Stop <kbd class="font-sans text-[10px] text-wb-ink-soft">Esc</kbd>
		</button>
	{/if}
</div>

<style>
	/* Three bars breathing out of phase: the one moving thing on screen while
	   Claude works. Amber and slower while it waits on you. */
	.bars {
		display: inline-flex;
		align-items: center;
		gap: 2px;
		height: 12px;
	}
	.bars i {
		width: 3px;
		height: 100%;
		border-radius: 2px;
		background: var(--wb-claude);
		transform-origin: center;
		animation: breathe 1.1s ease-in-out infinite;
	}
	.bars i:nth-child(2) {
		animation-delay: 0.15s;
	}
	.bars i:nth-child(3) {
		animation-delay: 0.3s;
	}
	.waiting .bars i {
		background: var(--wb-warn);
		animation-duration: 2.2s;
	}
	@keyframes breathe {
		0%,
		100% {
			transform: scaleY(0.35);
			opacity: 0.5;
		}
		50% {
			transform: scaleY(1);
			opacity: 1;
		}
	}

	/* A soft highlight sliding across the label. */
	.label {
		background: linear-gradient(
				90deg,
				var(--wb-ink-mute) 0%,
				var(--wb-ink) 45%,
				var(--wb-ink-mute) 90%
			)
			0 0 / 250% 100%;
		-webkit-background-clip: text;
		background-clip: text;
		color: transparent;
		animation: sheen 2.4s linear infinite;
	}
	@keyframes sheen {
		from {
			background-position: 125% 0;
		}
		to {
			background-position: -125% 0;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.bars i,
		.label {
			animation: none;
		}
		.bars i {
			transform: scaleY(0.7);
			opacity: 1;
		}
		.label {
			color: var(--wb-ink);
		}
	}
</style>
