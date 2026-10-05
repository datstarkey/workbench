<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn } from '@workbench/ui';
	import * as Popover from '@workbench/ui/popover';
	let {
		percent,
		label,
		value,
		warn,
		title,
		class: className,
		children,
		detail
	}: {
		percent: number;
		label?: string;
		/** Shown in place of the percentage. */
		value?: string;
		/** Defaults to 80% and over. */
		warn?: boolean;
		title: string;
		class?: string;
		children?: Snippet;
		detail: Snippet;
	} = $props();
	const fill = $derived(Math.max(0, Math.min(100, Number.isFinite(percent) ? percent : 0)));
	const warning = $derived(warn ?? fill >= 80);
</script>

<Popover.Root>
	<Popover.Trigger>
		{#snippet child({ props })}
			<button
				{...props}
				type="button"
				{title}
				aria-label={title}
				class={cn(
					'metric-pill relative flex h-7 shrink-0 items-center gap-1.5 rounded-full border border-wb-hair bg-wb-panel px-2.5 text-[11.5px] tabular-nums focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none',
					warning ? 'text-wb-warn' : 'text-wb-ink-soft',
					className
				)}
				style:--metric-color={warning ? 'var(--wb-warn)' : 'var(--wb-agent, var(--wb-accent))'}
			>
				<svg class="metric-border pointer-events-none absolute" aria-hidden="true">
					<rect pathLength="100" stroke-dasharray="{fill} 100" />
				</svg>
				{@render children?.()}
				{#if label}<span class="text-wb-ink-mute">{label}</span>{/if}
				{value ?? `${Math.round(percent)}%`}
			</button>
		{/snippet}
	</Popover.Trigger>
	<Popover.Content
		class="w-64 border-wb-hair bg-wb-panel p-3 text-xs text-wb-ink"
		side="top"
		sideOffset={8}
	>
		{@render detail()}
	</Popover.Content>
</Popover.Root>

<style>
	.metric-border {
		inset: -1px;
		width: calc(100% + 2px);
		height: calc(100% + 2px);
	}
	.metric-border rect {
		x: 0.5px;
		y: 0.5px;
		width: calc(100% - 1px);
		height: calc(100% - 1px);
		rx: 14px;
		fill: none;
		stroke: var(--metric-color);
		stroke-width: 1px;
	}
</style>
