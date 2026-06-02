<script lang="ts">
	import Columns2Icon from '@lucide/svelte/icons/columns-2';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Rows2Icon from '@lucide/svelte/icons/rows-2';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import XIcon from '@lucide/svelte/icons/x';
	import ZapIcon from '@lucide/svelte/icons/zap';
	import { Button } from '$lib/components/ui/button';
	import { Separator } from '$lib/components/ui/separator';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import AgentActionsMenu from '$features/agent-actions/AgentActionsMenu.svelte';
	import ClaudeSessionMenu from '$features/claude/ClaudeSessionMenu.svelte';
	import {
		getClaudeSessionStore,
		getProjectStore,
		getWorkbenchSettingsStore,
		getWorkspaceStore
	} from '$stores/context';
	import { effectivePath } from '$lib/utils/path';
	import type { ProjectWorkspace, TerminalTabState } from '$types/workbench';

	const workspaceStore = getWorkspaceStore();
	const claudeSessionStore = getClaudeSessionStore();
	const projectStore = getProjectStore();
	const workbenchSettings = getWorkbenchSettingsStore();
	let nativeMode = $derived(workbenchSettings.terminalRenderer === 'native');

	let {
		workspace
	}: {
		workspace: ProjectWorkspace;
	} = $props();

	let tabs = $derived(workspace.terminalTabs);
	let activeTabId = $derived(workspace.activeTerminalTabId);
	let wsProject = $derived(projectStore.getByPath(workspace.projectPath));
	let wsCwd = $derived(effectivePath(workspace));

	/** Map of tabId → session status for AI tabs */
	let sessionsByTabId = $derived.by(() => {
		const sessions = claudeSessionStore.activeSessionsByProject[workspace.projectPath] ?? [];
		return Object.fromEntries(sessions.map((s) => [s.tabId, s]));
	});

	/** Badge label: first 3 chars of type uppercased */
	function kindBadge(tab: TerminalTabState): string {
		const t = tab.type ?? 'shell';
		return t.slice(0, 3).toUpperCase();
	}

	/** Tailwind classes for the session-kind badge */
	function kindBadgeClass(tab: TerminalTabState): string {
		if (tab.type === 'claude') return 'bg-wb-claude/20 text-wb-claude';
		if (tab.type === 'codex') return 'bg-wb-codex/20 text-wb-codex';
		return 'bg-wb-shell/20 text-wb-shell';
	}

	/** Top border color for active tab based on session type */
	function activeTopBorderClass(tab: TerminalTabState): string {
		if (tab.type === 'claude') return 'bg-wb-claude';
		if (tab.type === 'codex') return 'bg-wb-codex';
		return 'bg-wb-shell';
	}
</script>

<div class="flex h-[32px] shrink-0 items-stretch border-b border-wb-hair bg-wb-panel">
	<div
		class="flex flex-1 items-stretch gap-0 overflow-x-auto"
		role="tablist"
		aria-label="Terminal tabs"
	>
		{#each tabs as tab, idx (tab.id)}
			{@const isActive = tab.id === activeTabId}
			{@const tabSession = sessionsByTabId[tab.id]}
			{@const isLive =
				tab.type === 'claude' || tab.type === 'codex'
					? tabSession != null && !tabSession.needsAttention && !tabSession.awaitingInput
					: false}
			{@const isAwaiting = tabSession?.awaitingInput ?? false}
			<div
				class={[
					'group relative inline-flex items-stretch border-r border-wb-hair transition-colors',
					isActive ? 'bg-wb-bg' : 'bg-transparent hover:bg-wb-panel2/60'
				]}
				role="presentation"
			>
				<!-- Top accent border (2px) for active tab -->
				{#if isActive}
					<span class={['absolute inset-x-0 top-0 h-0.5', activeTopBorderClass(tab)]}></span>
				{/if}
				<button
					class={[
						'flex items-center gap-1.5 px-2.5 font-mono text-[11.5px] whitespace-nowrap',
						isActive ? 'text-wb-ink' : 'text-wb-ink-mute'
					]}
					type="button"
					role="tab"
					aria-selected={isActive}
					onclick={() => workspaceStore.setActiveTab(workspace.id, tab.id)}
				>
					<!-- Index number -->
					<span class="text-[9.5px] text-wb-ink-soft">{idx + 1}</span>
					<!-- Kind badge -->
					<span
						class={[
							'rounded px-1 font-mono text-[9.5px] font-bold tracking-wide',
							kindBadgeClass(tab)
						]}>{kindBadge(tab)}</span
					>
					<!-- Label -->
					{tab.label}
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

	<div class="flex shrink-0 items-center gap-0.5 border-l border-wb-hair px-1">
		<Tooltip.Root>
			<Tooltip.Trigger>
				<Button
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
			</Tooltip.Trigger>
			<Tooltip.Content>New Terminal</Tooltip.Content>
		</Tooltip.Root>

		<AgentActionsMenu {workspace} />

		<Tooltip.Root>
			<Tooltip.Trigger>
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-6 text-wb-claude hover:bg-wb-claude/10 hover:text-wb-claude"
					type="button"
					onclick={() => claudeSessionStore.startSessionInWorkspace(workspace)}
				>
					<SparklesIcon class="size-3.5" />
				</Button>
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
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-6 text-wb-codex hover:bg-wb-codex/10 hover:text-wb-codex"
					type="button"
					onclick={() => claudeSessionStore.startSessionInWorkspace(workspace, 'codex')}
				>
					<ZapIcon class="size-3.5" />
				</Button>
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
					<Button
						variant="ghost"
						size="icon-sm"
						class="size-6 text-wb-ink-soft hover:bg-wb-panel2 hover:text-wb-ink"
						type="button"
						onclick={() => workspaceStore.splitTerminal(workspace.id, 'horizontal')}
					>
						<Columns2Icon class="size-3.5" />
					</Button>
				</Tooltip.Trigger>
				<Tooltip.Content>Split Horizontal</Tooltip.Content>
			</Tooltip.Root>

			<Tooltip.Root>
				<Tooltip.Trigger>
					<Button
						variant="ghost"
						size="icon-sm"
						class="size-6 text-wb-ink-soft hover:bg-wb-panel2 hover:text-wb-ink"
						type="button"
						onclick={() => workspaceStore.splitTerminal(workspace.id, 'vertical')}
					>
						<Rows2Icon class="size-3.5" />
					</Button>
				</Tooltip.Trigger>
				<Tooltip.Content>Split Vertical</Tooltip.Content>
			</Tooltip.Root>
		{/if}
	</div>
</div>
