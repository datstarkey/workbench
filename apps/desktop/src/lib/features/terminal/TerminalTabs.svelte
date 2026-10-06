<script lang="ts">
	import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
	import { AgentIcon } from '@workbench/ui/agent-icon';
	import Columns2Icon from '@lucide/svelte/icons/columns-2';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Rows2Icon from '@lucide/svelte/icons/rows-2';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '@workbench/ui/button';
	import { Separator } from '@workbench/ui/separator';
	import * as Tooltip from '@workbench/ui/tooltip';
	import AgentActionsMenu from '$features/agent-actions/AgentActionsMenu.svelte';
	import ClaudeSessionMenu from '$features/claude/ClaudeSessionMenu.svelte';
	import { getClaudeSessionStore, getProjectStore, getWorkspaceStore } from '$stores/context';
	import { overlayScrollbars } from '$lib/utils/overlay-scrollbars';
	import { effectivePath } from '$lib/utils/path';
	import { TabReorder } from '$lib/utils/tab-reorder.svelte';
	import { visibleSplit } from './split-view';
	import type { ProjectWorkspace, SplitDirection, TerminalTabState } from '$types/workbench';

	const workspaceStore = getWorkspaceStore();
	const claudeSessionStore = getClaudeSessionStore();
	const projectStore = getProjectStore();

	let {
		workspace
	}: {
		workspace: ProjectWorkspace;
	} = $props();

	let tabs = $derived(workspace.terminalTabs);
	let activeTabId = $derived(workspace.activeTerminalTabId);
	let wsProject = $derived(projectStore.getByPath(workspace.projectPath));
	let wsCwd = $derived(effectivePath(workspace));
	let split = $derived(visibleSplit(workspace));
	// Per workspace, like the grid App.svelte renders: native panes are OS views CSS can't split.
	let nativeMode = $derived(workspace.renderer === 'native');

	const reorder = new TabReorder(
		() => tabs.map((t) => t.id),
		(fromId, toId) => workspaceStore.reorderTerminalTab(workspace.id, fromId, toId)
	);

	function toggleSplit(direction: SplitDirection) {
		if (wsProject) workspaceStore.splitTerminal(workspace.id, direction, wsProject);
	}

	/** Map of tabId → session status for AI tabs */
	let sessionsByTabId = $derived.by(() => {
		const sessions = claudeSessionStore.activeSessionsByProject[workspace.projectPath] ?? [];
		return Object.fromEntries(sessions.map((s) => [s.tabId, s]));
	});

	/** Top border color for active tab based on session type */
	function activeTopBorderClass(tab: TerminalTabState): string {
		if (tab.type === 'claude') return 'bg-wb-claude';
		if (tab.type === 'codex') return 'bg-wb-codex';
		return 'bg-wb-shell';
	}
</script>

<div class="flex h-[32px] shrink-0 items-stretch border-b border-wb-hair bg-wb-panel">
	<div
		class="min-w-0 flex-1"
		{@attach overlayScrollbars({ overflow: { x: 'scroll', y: 'hidden' } })}
	>
		<div class="flex h-full items-stretch gap-0" role="tablist" aria-label="Terminal tabs">
			{#each tabs as tab, idx (tab.id)}
				{@const isActive = tab.id === activeTabId}
				{@const tabSession = sessionsByTabId[tab.id]}
				{@const isLive =
					tab.type === 'claude' || tab.type === 'codex'
						? tabSession != null && !tabSession.needsAttention && !tabSession.awaitingInput
						: false}
				{@const isAwaiting = tabSession?.awaitingInput ?? false}
				{@const inSplit = split?.tabs.some((t) => t.id === tab.id) ?? false}
				{@const dropSide = reorder.dropSide(tab.id)}
				<div
					class={[
						'group relative inline-flex items-stretch border-r border-wb-hair transition-colors',
						isActive
							? 'bg-wb-bg'
							: inSplit
								? 'bg-wb-bg/50'
								: 'bg-transparent hover:bg-wb-panel2/60',
						reorder.dragging === tab.id && 'opacity-50'
					]}
					role="presentation"
					{...reorder.handlers(tab.id)}
				>
					<!-- Top accent border (2px) for active tab -->
					{#if isActive}
						<span class={['absolute inset-x-0 top-0 h-0.5', activeTopBorderClass(tab)]}></span>
					{/if}
					{#if dropSide}
						<span
							class={[
								'absolute inset-y-1 z-10 w-0.5 rounded bg-wb-accent',
								dropSide === 'before' ? '-left-px' : '-right-px'
							]}
						></span>
					{/if}
					<button
						class={[
							'flex items-center gap-1.5 px-2.5 font-mono text-[11.5px] whitespace-nowrap',
							isActive ? 'text-wb-ink' : 'text-wb-ink-mute'
						]}
						type="button"
						role="tab"
						aria-selected={isActive}
						title={tab.label}
						onclick={() => workspaceStore.setActiveTab(workspace.id, tab.id)}
					>
						<!-- Index number -->
						<span class="text-[9.5px] text-wb-ink-soft">{idx + 1}</span>
						{#if tab.type === 'claude' || tab.type === 'codex'}
							<AgentIcon agent={tab.type} class="size-3.5" />
							<span class="sr-only">{tab.type === 'claude' ? 'Claude' : 'Codex'}:</span>
						{:else}
							<SquareTerminalIcon class="size-3.5 text-wb-shell" />
							<span class="sr-only">Terminal:</span>
						{/if}
						<span class="max-w-36 truncate">{tab.label}</span>
						<!-- Status indicator -->
						{#if isLive}
							<span class="wb-pulse size-1.5 shrink-0 rounded-full bg-wb-ok"></span>
						{:else if isAwaiting}
							<span class="text-[9.5px] font-bold text-wb-warn">?</span>
						{/if}
					</button>
					<button
						class="mr-0.5 flex size-4 shrink-0 items-center justify-center self-center rounded text-wb-ink-soft opacity-0 transition-opacity group-hover:opacity-100 hover:bg-wb-panel2 hover:text-wb-ink"
						type="button"
						aria-label="Close terminal tab"
						onclick={() => workspaceStore.closeTerminalTab(workspace.id, tab.id)}
					>
						<XIcon class="size-3" />
					</button>
				</div>
			{/each}
		</div>
	</div>

	<div class="flex shrink-0 items-center gap-0.5 border-l border-wb-hair px-1">
		<Tooltip.Root>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<Button
						{...props}
						variant="ghost"
						size="icon-sm"
						class="size-6 text-wb-ink-soft hover:bg-wb-panel2 hover:text-wb-ink"
						type="button"
						onclick={() => {
							if (wsProject) workspaceStore.addTerminalTab(workspace.id, wsProject);
						}}
					>
						<PlusIcon class="size-3.5" />
					</Button>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Content>New Terminal</Tooltip.Content>
		</Tooltip.Root>

		<AgentActionsMenu {workspace} />

		<Tooltip.Root>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<Button
						{...props}
						variant="ghost"
						size="icon-sm"
						class="size-6 text-wb-claude hover:bg-wb-claude/10 hover:text-wb-claude"
						type="button"
						onclick={() => claudeSessionStore.startSessionInWorkspace(workspace)}
					>
						<AgentIcon agent="claude" class="size-3.5" />
					</Button>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Content>New Claude Session</Tooltip.Content>
		</Tooltip.Root>

		<ClaudeSessionMenu
			type="claude"
			onResume={(sessionId, label) =>
				claudeSessionStore.resumeSession(workspace.id, sessionId, label)}
			onOpen={() => claudeSessionStore.discoverSessions(wsCwd)}
		/>

		<Tooltip.Root>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<Button
						{...props}
						variant="ghost"
						size="icon-sm"
						class="size-6 text-wb-codex hover:bg-wb-codex/10 hover:text-wb-codex"
						type="button"
						onclick={() => claudeSessionStore.startSessionInWorkspace(workspace, 'codex')}
					>
						<AgentIcon agent="codex" class="size-3.5" />
					</Button>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Content>New Codex Session</Tooltip.Content>
		</Tooltip.Root>

		<ClaudeSessionMenu
			type="codex"
			onResume={(sessionId, label) =>
				claudeSessionStore.resumeSession(workspace.id, sessionId, label, 'codex')}
			onOpen={() => claudeSessionStore.discoverCodexSessions(wsCwd)}
		/>

		{#if !nativeMode}
			<Separator orientation="vertical" class="!h-4" />

			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<Button
							{...props}
							variant="ghost"
							size="icon-sm"
							class={[
								'size-6 hover:bg-wb-panel2 hover:text-wb-ink',
								split?.direction === 'horizontal' ? 'bg-wb-panel2 text-wb-ink' : 'text-wb-ink-soft'
							]}
							type="button"
							aria-pressed={split?.direction === 'horizontal'}
							onclick={() => toggleSplit('horizontal')}
						>
							<Columns2Icon class="size-3.5" />
						</Button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content
					>{split?.direction === 'horizontal' ? 'Unsplit' : 'Split Horizontal'}</Tooltip.Content
				>
			</Tooltip.Root>

			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<Button
							{...props}
							variant="ghost"
							size="icon-sm"
							class={[
								'size-6 hover:bg-wb-panel2 hover:text-wb-ink',
								split?.direction === 'vertical' ? 'bg-wb-panel2 text-wb-ink' : 'text-wb-ink-soft'
							]}
							type="button"
							aria-pressed={split?.direction === 'vertical'}
							onclick={() => toggleSplit('vertical')}
						>
							<Rows2Icon class="size-3.5" />
						</Button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content
					>{split?.direction === 'vertical' ? 'Unsplit' : 'Split Vertical'}</Tooltip.Content
				>
			</Tooltip.Root>
		{/if}
	</div>
</div>
