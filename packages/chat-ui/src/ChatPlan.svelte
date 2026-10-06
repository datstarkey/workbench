<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import { cn } from '@workbench/ui';
	import type { TodoStep } from './chat-format';

	let { steps }: { steps: TodoStep[] } = $props();

	const done = $derived(steps.filter((s) => s.status === 'completed').length);
	const current = $derived(steps.find((s) => s.status === 'in_progress'));
	const label = $derived(current?.activeForm ?? current?.content ?? '');
</script>

<details class="group rounded-lg border border-wb-hair bg-wb-panel text-xs">
	<summary
		class="flex cursor-pointer list-none items-center gap-3 px-3 py-2 select-none focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
	>
		<span class="shrink-0 text-wb-ink-mute tabular-nums">Plan {done}/{steps.length}</span>
		<span class="h-1 w-16 shrink-0 overflow-hidden rounded-full bg-wb-panel2">
			<span
				class="block h-full rounded-full bg-wb-ok transition-[width] duration-500"
				style:width="{(done / steps.length) * 100}%"
			></span>
		</span>
		<span class="min-w-0 truncate text-wb-ink">{label}</span>
	</summary>
	<ol class="flex flex-col gap-1 border-t border-wb-hair px-3 py-2">
		{#each steps as step, i (i)}
			<li
				class={cn(
					'flex items-start gap-2',
					step.status === 'completed' && 'text-wb-ink-soft',
					step.status === 'in_progress' && 'text-wb-ink',
					step.status === 'pending' && 'text-wb-ink-mute'
				)}
			>
				<span
					class={cn(
						'mt-0.5 flex size-3 shrink-0 items-center justify-center rounded-[3px] border',
						step.status === 'completed' && 'border-wb-ok bg-wb-ok text-wb-accent-ink',
						step.status === 'in_progress' && 'border-[var(--wb-agent,var(--wb-claude))]',
						step.status === 'pending' && 'border-wb-ink-soft'
					)}
				>
					{#if step.status === 'completed'}<CheckIcon class="size-2.5" />{/if}
				</span>
				<span class={cn(step.status === 'completed' && 'line-through')}>{step.content}</span>
			</li>
		{/each}
	</ol>
</details>
