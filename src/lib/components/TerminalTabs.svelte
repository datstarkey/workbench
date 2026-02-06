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
	import ClaudeSessionMenu from '$components/ClaudeSessionMenu.svelte';
	import type { DiscoveredClaudeSession, TerminalTabState } from '$types/workbench';

	let {
		tabs,
		activeTabId,
		discoveredSessions,
		onSelect,
		onClose,
		onAdd,
		onAddClaude,
		onResumeClaude,
		onDiscoverSessions,
		onSplitHorizontal,
		onSplitVertical
	}: {
		tabs: TerminalTabState[];
		activeTabId: string;
		discoveredSessions: DiscoveredClaudeSession[];
		onSelect: (id: string) => void;
		onClose: (id: string) => void;
		onAdd: () => void;
		onAddClaude: () => void;
		onResumeClaude: (sessionId: string, label: string) => void;
		onDiscoverSessions: () => void;
		onSplitHorizontal: () => void;
		onSplitVertical: () => void;
	} = $props();
</script>

<div class="flex h-9 shrink-0 items-center border-b border-border/60 px-1">
	<div class="flex flex-1 items-center gap-0.5 overflow-x-auto">
		{#each tabs as tab (tab.id)}
			{@const isActive = tab.id === activeTabId}
			{@const isClaude = tab.type === 'claude'}
			<div
				class={`inline-flex items-center rounded-md transition-colors ${isActive ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground'}`}
			>
				<button
					class="flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium"
					type="button"
					onclick={() => onSelect(tab.id)}
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
						onclick={() => onClose(tab.id)}
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
					onclick={onAdd}
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
					onclick={onAddClaude}
				>
					<SparklesIcon class="size-3.5" />
				</Button>
			</Tooltip.Trigger>
			<Tooltip.Content>New Claude Session</Tooltip.Content>
		</Tooltip.Root>

		<ClaudeSessionMenu
			sessions={discoveredSessions}
			onResume={onResumeClaude}
			onOpen={onDiscoverSessions}
		/>

		<Separator orientation="vertical" class="!h-4" />

		<Tooltip.Root>
			<Tooltip.Trigger>
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-7 text-muted-foreground hover:text-foreground"
					type="button"
					onclick={onSplitHorizontal}
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
					onclick={onSplitVertical}
				>
					<Rows2Icon class="size-3.5" />
				</Button>
			</Tooltip.Trigger>
			<Tooltip.Content>Split Vertical</Tooltip.Content>
		</Tooltip.Root>
	</div>
</div>
