<script lang="ts">
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import LinkIcon from '@lucide/svelte/icons/link';
	import UnlinkIcon from '@lucide/svelte/icons/unlink';
	import GitBranchPlusIcon from '@lucide/svelte/icons/git-branch-plus';
	import type { TrelloCard } from '$types/trello';
	import { getTrelloStore, getWorktreeManager } from '$stores/context';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { invoke } from '@tauri-apps/api/core';
	import { trelloLabelColor } from './label-colors';

	let {
		card,
		projectPath,
		boardId
	}: { card: TrelloCard; projectPath: string | null; boardId: string } = $props();

	const trelloStore = getTrelloStore();
	const worktreeManager = getWorktreeManager();

	let isLinked = $derived(trelloStore.activeTaskLinks.some((t) => t.cardId === card.id));
	let linkedBranch = $derived(
		trelloStore.activeTaskLinks.find((t) => t.cardId === card.id)?.branch ?? null
	);

	function handleOpenInTrello() {
		invoke('open_url', { url: card.url });
	}

	function handleCreateWorktree() {
		if (!projectPath) return;
		const branchName = card.name
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-|-$/g, '')
			.slice(0, 50);
		worktreeManager.addWithBranch(projectPath, branchName, card.id, boardId);
	}

	function handleLinkToBranch() {
		if (!projectPath || !trelloStore.activeBranch) return;
		trelloStore.linkTaskToBranch(projectPath, card.id, boardId, trelloStore.activeBranch);
	}

	function handleUnlink() {
		if (!projectPath) return;
		trelloStore.unlinkTask(projectPath, card.id);
	}
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger>
		{#snippet child({ props })}
			<button
				{...props}
				class="group flex w-full items-start gap-2 rounded-md border border-border/50 bg-card px-2.5 py-2 text-left shadow-sm hover:border-border hover:bg-accent/30"
			>
				<div class="min-w-0 flex-1">
					<Tooltip.Root>
						<Tooltip.Trigger>
							<p class="truncate text-left text-xs">{card.name}</p>
						</Tooltip.Trigger>
						<Tooltip.Content side="left">{card.name}</Tooltip.Content>
					</Tooltip.Root>
					{#if card.labels.length > 0}
						<div class="mt-1 flex flex-wrap gap-1">
							{#each card.labels as label (label.id)}
								<span
									class="inline-block h-1.5 w-5 rounded-full"
									style="background-color: {trelloLabelColor(label.color)}"
									title={label.name}
								></span>
							{/each}
						</div>
					{/if}
					{#if isLinked}
						<Badge variant="outline" class="mt-1 h-4 gap-0.5 text-[9px]">
							<LinkIcon class="size-2" />
							{linkedBranch}
						</Badge>
					{/if}
				</div>
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-5 shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
					onclick={(e) => {
						e.stopPropagation();
						handleOpenInTrello();
					}}
				>
					<ExternalLinkIcon class="size-3" />
				</Button>
			</button>
		{/snippet}
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="start" class="w-48">
		<DropdownMenu.Item onclick={handleCreateWorktree}>
			<GitBranchPlusIcon class="mr-2 size-3.5" />
			Create Worktree
		</DropdownMenu.Item>
		{#if !isLinked}
			<DropdownMenu.Item onclick={handleLinkToBranch} disabled={!trelloStore.activeBranch}>
				<LinkIcon class="mr-2 size-3.5" />
				Link to Current Branch
			</DropdownMenu.Item>
		{:else}
			<DropdownMenu.Item onclick={handleUnlink}>
				<UnlinkIcon class="mr-2 size-3.5" />
				Unlink
			</DropdownMenu.Item>
		{/if}
		<DropdownMenu.Separator />
		<DropdownMenu.Item onclick={handleOpenInTrello}>
			<ExternalLinkIcon class="mr-2 size-3.5" />
			Open in Trello
		</DropdownMenu.Item>
	</DropdownMenu.Content>
</DropdownMenu.Root>
