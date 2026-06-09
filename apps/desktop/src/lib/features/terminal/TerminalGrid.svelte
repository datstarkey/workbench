<script lang="ts">
	import XIcon from '@lucide/svelte/icons/x';
	import TerminalPane from '$features/terminal/TerminalPane.svelte';
	import { getWorkspaceStore } from '$stores/context';
	import type { ProjectConfig, SplitDirection, TerminalPaneState } from '$types/workbench';

	const workspaceStore = getWorkspaceStore();

	let {
		workspaceId,
		panes,
		split,
		active,
		project,
		cwd
	}: {
		workspaceId: string;
		panes: TerminalPaneState[];
		split: SplitDirection;
		active: boolean;
		project: ProjectConfig;
		cwd?: string;
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
			<TerminalPane
				sessionId={pane.id}
				{project}
				{active}
				{cwd}
				startupCommand={pane.startupCommand}
			/>
			{#if panes.length > 1}
				<button
					class="absolute top-2 right-2 flex size-6 items-center justify-center rounded bg-background/80 text-muted-foreground opacity-0 backdrop-blur-sm transition-opacity hover:text-foreground [div:hover>&]:opacity-100"
					type="button"
					aria-label="Close pane"
					onclick={() => workspaceStore.removePane(workspaceId, pane.id)}
				>
					<XIcon class="size-3" />
				</button>
			{/if}
		</div>
	{/each}
</div>
