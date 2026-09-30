<script lang="ts">
	import XIcon from '@lucide/svelte/icons/x';
	import { cn } from '@workbench/ui';
	import SessionChat from '$features/chat/SessionChat.svelte';
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
		{@const chatSessionId = pane.type === 'claude' ? pane.claudeSessionId : undefined}
		{#if i > 0}
			<div
				class={split === 'vertical' ? 'h-px shrink-0 bg-border/60' : 'w-px shrink-0 bg-border/60'}
			></div>
		{/if}
		<div class="relative min-h-0 min-w-0 flex-1">
			<!-- The terminal stays mounted under the chat so the session never stops. -->
			<TerminalPane
				sessionId={pane.id}
				{project}
				{active}
				{cwd}
				startupCommand={pane.startupCommand}
				existingServerTerminalId={workspaceStore.getServerTerminalId(pane.id)}
				onServerTerminalIdChange={(paneId, serverTerminalId) =>
					workspaceStore.setServerTerminalId(paneId, serverTerminalId)}
			/>
			{#if chatSessionId && pane.view === 'chat'}
				<div class="absolute inset-0 z-10">
					{#key chatSessionId}
						<SessionChat
							paneId={pane.id}
							sessionId={chatSessionId}
							cwd={cwd ?? project.path}
							onShowTerminal={() => workspaceStore.setPaneView(pane.id, 'terminal')}
						/>
					{/key}
				</div>
			{/if}
			{#if chatSessionId}
				<div
					class="absolute top-1.5 right-10 z-20 flex overflow-hidden rounded-md border border-wb-hair bg-wb-panel/90 text-[11px] backdrop-blur-sm"
					role="group"
					aria-label="Session view"
				>
					{#each ['terminal', 'chat'] as const as view (view)}
						{@const selected = (pane.view ?? 'terminal') === view}
						<button
							type="button"
							class={cn(
								'px-2 py-0.5 capitalize',
								selected ? 'bg-wb-accent-soft text-wb-ink' : 'text-wb-ink-mute hover:text-wb-ink'
							)}
							aria-pressed={selected}
							onclick={() => workspaceStore.setPaneView(pane.id, view)}
						>
							{view}
						</button>
					{/each}
				</div>
			{/if}
			{#if panes.length > 1}
				<button
					class="absolute top-2 right-2 z-20 flex size-6 items-center justify-center rounded bg-background/80 text-muted-foreground opacity-0 backdrop-blur-sm transition-opacity hover:text-foreground [div:hover>&]:opacity-100"
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
