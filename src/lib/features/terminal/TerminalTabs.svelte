<script lang="ts">
	import Columns2Icon from '@lucide/svelte/icons/columns-2';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Rows2Icon from '@lucide/svelte/icons/rows-2';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import TerminalIcon from '@lucide/svelte/icons/terminal';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import { Separator } from '$lib/components/ui/separator';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import ClaudeSessionMenu from '$features/claude/ClaudeSessionMenu.svelte';
	import { getClaudeSessionStore, getProjectStore, getWorkspaceStore } from '$stores/context';
	import { effectivePath } from '$lib/utils/path';
	import type { ProjectWorkspace } from '$types/workbench';

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
</script>

<div class="flex h-9 shrink-0 items-center border-b border-border/60 px-1">
	<div
		class="flex flex-1 items-center gap-0.5 overflow-x-auto"
		role="tablist"
		aria-label="Terminal tabs"
	>
		{#each tabs as tab (tab.id)}
			{@const isActive = tab.id === activeTabId}
			{@const isClaude = tab.type === 'claude'}
			<div
				class={`inline-flex items-center rounded-md transition-colors ${isActive ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground'}`}
				role="presentation"
			>
				<button
					class="flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium"
					type="button"
					role="tab"
					aria-selected={isActive}
					onclick={() => workspaceStore.setActiveTab(workspace.id, tab.id)}
				>
					{#if isClaude}
						<SparklesIcon class="size-3 text-violet-400" />
					{:else}
						<TerminalIcon class="size-3" />
					{/if}
					{tab.label}
				</button>
				{#if tabs.length > 1}
					<button
						class="mr-0.5 flex size-5 items-center justify-center rounded opacity-50 transition-opacity hover:bg-muted hover:opacity-100"
						type="button"
						aria-label="Close terminal tab"
						onclick={() => workspaceStore.closeTerminalTab(workspace.id, tab.id)}
					>
						<XIcon class="size-3" />
					</button>
				{/if}
			</div>
		{/each}
	</div>

	<div class="flex shrink-0 items-center gap-0.5 border-l border-border/60 pr-1 pl-2">
		<Tooltip.Root>
			<Tooltip.Trigger>
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-7 text-muted-foreground hover:text-foreground"
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

		<Tooltip.Root>
			<Tooltip.Trigger>
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-7 text-violet-400 hover:text-violet-300"
					type="button"
					onclick={() => claudeSessionStore.startSessionInWorkspace(workspace)}
				>
					<SparklesIcon class="size-3.5" />
				</Button>
			</Tooltip.Trigger>
			<Tooltip.Content>New Claude Session</Tooltip.Content>
		</Tooltip.Root>

		<ClaudeSessionMenu
			onResume={(sessionId, label) =>
				workspaceStore.resumeClaudeSession(workspace.id, sessionId, label)}
			onOpen={() => claudeSessionStore.discoverSessions(wsCwd)}
		/>

		<Separator orientation="vertical" class="!h-4" />

		<Tooltip.Root>
			<Tooltip.Trigger>
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-7 text-muted-foreground hover:text-foreground"
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
					class="size-7 text-muted-foreground hover:text-foreground"
					type="button"
					onclick={() => workspaceStore.splitTerminal(workspace.id, 'vertical')}
				>
					<Rows2Icon class="size-3.5" />
				</Button>
			</Tooltip.Trigger>
			<Tooltip.Content>Split Vertical</Tooltip.Content>
		</Tooltip.Root>
	</div>
</div>
