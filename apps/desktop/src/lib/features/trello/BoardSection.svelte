<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { Button } from '$lib/components/ui/button';
	import { Separator } from '$lib/components/ui/separator';
	import type { TrelloBoardData } from '$types/trello';
	import TaskCard from './TaskCard.svelte';
	import QuickAddTask from './QuickAddTask.svelte';

	let { boardData, projectPath }: { boardData: TrelloBoardData; projectPath: string | null } =
		$props();

	let collapsed = $state(false);
</script>

<div>
	<!-- Board header -->
	<Button
		variant="ghost"
		size="sm"
		class="h-7 w-full justify-start gap-1.5 text-xs font-medium"
		onclick={() => (collapsed = !collapsed)}
	>
		{#if collapsed}
			<ChevronRightIcon class="size-3 text-muted-foreground" />
		{:else}
			<ChevronDownIcon class="size-3 text-muted-foreground" />
		{/if}
		<span class="truncate">{boardData.board.name}</span>
		<span class="ml-auto text-[10px] text-muted-foreground">
			{boardData.columns.reduce((n, c) => n + c.cards.length, 0)}
		</span>
	</Button>

	{#if !collapsed}
		<div class="mt-1 space-y-3 pl-1">
			{#each boardData.columns as col, i (col.column.id)}
				{#if i > 0}
					<Separator class="ml-2 w-[calc(100%-1rem)]" />
				{/if}
				<div>
					<!-- Column header -->
					<div class="flex items-center gap-1.5 px-2 py-0.5">
						<span class="text-[10px] font-semibold tracking-wider text-muted-foreground uppercase">
							{col.column.name}
						</span>
						<span class="text-[10px] text-muted-foreground/50">{col.cards.length}</span>
					</div>

					<!-- Cards -->
					{#if col.cards.length > 0}
						<div class="mt-1 space-y-1.5">
							{#each col.cards as card (card.id)}
								<TaskCard {card} {projectPath} boardId={boardData.board.id} />
							{/each}
						</div>
					{/if}

					<!-- Quick add -->
					{#if projectPath}
						<QuickAddTask listId={col.column.id} {projectPath} />
					{/if}
				</div>
			{/each}
		</div>
	{/if}
</div>
