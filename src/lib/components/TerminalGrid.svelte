<script lang="ts">
	import XIcon from '@lucide/svelte/icons/x';
	import TerminalPane from '$components/TerminalPane.svelte';
	import type { ProjectConfig, SplitDirection, TerminalPaneState } from '$types/workbench';

	let {
		panes,
		split,
		active,
		project,
		onRemovePane
	}: {
		panes: TerminalPaneState[];
		split: SplitDirection;
		active: boolean;
		project: ProjectConfig;
		onRemovePane: (paneId: string) => void;
	} = $props();
</script>

<div class={`flex min-h-0 flex-1 ${split === 'vertical' ? 'flex-col' : 'flex-row'}`}>
	{#each panes as pane, i (pane.id)}
		{#if i > 0}
			<div
				class={split === 'vertical' ? 'h-px shrink-0 bg-border/60' : 'w-px shrink-0 bg-border/60'}
			></div>
		{/if}
		<div class="relative min-h-0 min-w-0 flex-1">
			<TerminalPane sessionId={pane.id} {project} {active} startupCommand={pane.startupCommand} />
			{#if panes.length > 1}
				<button
					class="absolute top-2 right-2 flex size-6 items-center justify-center rounded bg-background/80 text-muted-foreground opacity-0 backdrop-blur-sm transition-opacity hover:text-foreground [div:hover>&]:opacity-100"
					type="button"
					aria-label="Close pane"
					onclick={() => onRemovePane(pane.id)}
				>
					<XIcon class="size-3" />
				</button>
			{/if}
		</div>
	{/each}
</div>
