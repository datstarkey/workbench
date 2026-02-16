<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Separator } from '$lib/components/ui/separator';
	import { getTrelloStore } from '$stores/context';
	import type { BoardConfig, MergeAction, TrelloLabel, TrelloList } from '$types/trello';
	import { trelloLabelColor } from '$features/trello/label-colors';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import TrashIcon from '@lucide/svelte/icons/trash-2';
	import XIcon from '@lucide/svelte/icons/x';
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';

	let { projectPath }: { projectPath: string } = $props();

	const trelloStore = getTrelloStore();

	let boards = $derived(trelloStore.configByProject[projectPath]?.boards ?? []);
	let configuredBoardIds = $derived(new Set(boards.map((b) => b.boardId)));
	let pickableBoards = $derived(
		trelloStore.availableBoards.filter((b) => !configuredBoardIds.has(b.id))
	);

	let showPicker = $state(false);
	let loadingPicker = $state(false);

	// Per-board column/label data for config UI
	let columnsByBoard: Record<string, TrelloList[]> = $state({});
	let labelsByBoard: Record<string, TrelloLabel[]> = $state({});

	onMount(() => {
		trelloStore.loadProjectConfig(projectPath);
	});

	async function handleShowPicker() {
		showPicker = true;
		loadingPicker = true;
		await trelloStore.fetchAvailableBoards();
		loadingPicker = false;
	}

	async function handleAddBoard(boardId: string, boardName: string) {
		const config = trelloStore.configByProject[projectPath] ?? { boards: [], taskLinks: [] };
		if (config.boards.some((b) => b.boardId === boardId)) return;

		const newBoard: BoardConfig = { boardId, boardName, hiddenColumns: [] };
		await trelloStore.saveProjectConfig(projectPath, {
			...config,
			boards: [...config.boards, newBoard]
		});
		showPicker = false;
		loadColumnsAndLabels(boardId);
	}

	async function handleRemoveBoard(boardId: string) {
		const config = trelloStore.configByProject[projectPath];
		if (!config) return;
		await trelloStore.saveProjectConfig(projectPath, {
			...config,
			boards: config.boards.filter((b) => b.boardId !== boardId),
			taskLinks: config.taskLinks.filter((t) => t.boardId !== boardId)
		});
	}

	async function loadColumnsAndLabels(boardId: string) {
		try {
			const [columns, labels] = await Promise.all([
				invoke<TrelloList[]>('trello_list_columns', { boardId }),
				invoke<TrelloLabel[]>('trello_list_labels', { boardId })
			]);
			columnsByBoard = { ...columnsByBoard, [boardId]: columns };
			labelsByBoard = { ...labelsByBoard, [boardId]: labels };
		} catch (e) {
			console.warn('[SettingsBoardConfig] Failed to load columns/labels:', e);
		}
	}

	function toggleColumn(boardId: string, columnId: string) {
		const config = trelloStore.configByProject[projectPath];
		if (!config) return;
		const board = config.boards.find((b) => b.boardId === boardId);
		if (!board) return;

		const hidden = board.hiddenColumns.includes(columnId)
			? board.hiddenColumns.filter((c) => c !== columnId)
			: [...board.hiddenColumns, columnId];

		const updated = {
			...config,
			boards: config.boards.map((b) =>
				b.boardId === boardId ? { ...b, hiddenColumns: hidden } : b
			)
		};
		trelloStore.saveProjectConfig(projectPath, updated);
	}

	function updateBoardAction(
		boardId: string,
		field: 'linkAction' | 'mergeAction',
		action: MergeAction | undefined
	) {
		const config = trelloStore.configByProject[projectPath];
		if (!config) return;
		const updated = {
			...config,
			boards: config.boards.map((b) => (b.boardId === boardId ? { ...b, [field]: action } : b))
		};
		trelloStore.saveProjectConfig(projectPath, updated);
	}

	function defaultAction(): MergeAction {
		return { addLabelIds: [], removeLabelIds: [] };
	}

	function setActionColumn(
		boardId: string,
		field: 'linkAction' | 'mergeAction',
		current: MergeAction | undefined,
		columnId: string
	) {
		const col = columnsByBoard[boardId]?.find((c) => c.id === columnId);
		updateBoardAction(boardId, field, {
			...(current ?? defaultAction()),
			moveToColumnId: col?.id,
			moveToColumnName: col?.name
		});
	}

	function addActionLabel(
		boardId: string,
		field: 'linkAction' | 'mergeAction',
		current: MergeAction | undefined,
		labelId: string
	) {
		if (!labelId) return;
		const base = current ?? defaultAction();
		if (base.addLabelIds.includes(labelId)) return;
		updateBoardAction(boardId, field, {
			...base,
			addLabelIds: [...base.addLabelIds, labelId]
		});
	}

	function removeFromAddLabels(
		boardId: string,
		field: 'linkAction' | 'mergeAction',
		current: MergeAction | undefined,
		labelId: string
	) {
		const base = current ?? defaultAction();
		updateBoardAction(boardId, field, {
			...base,
			addLabelIds: base.addLabelIds.filter((id) => id !== labelId)
		});
	}

	function addRemoveLabel(
		boardId: string,
		field: 'linkAction' | 'mergeAction',
		current: MergeAction | undefined,
		labelId: string
	) {
		if (!labelId) return;
		const base = current ?? defaultAction();
		if (base.removeLabelIds.includes(labelId)) return;
		updateBoardAction(boardId, field, {
			...base,
			removeLabelIds: [...base.removeLabelIds, labelId]
		});
	}

	function removeFromRemoveLabels(
		boardId: string,
		field: 'linkAction' | 'mergeAction',
		current: MergeAction | undefined,
		labelId: string
	) {
		const base = current ?? defaultAction();
		updateBoardAction(boardId, field, {
			...base,
			removeLabelIds: base.removeLabelIds.filter((id) => id !== labelId)
		});
	}

	function getLabelById(boardId: string, labelId: string): TrelloLabel | undefined {
		return labelsByBoard[boardId]?.find((l) => l.id === labelId);
	}

	// Load columns/labels for already-configured boards (network side effect)
	$effect(() => {
		for (const board of boards) {
			if (!columnsByBoard[board.boardId]) {
				loadColumnsAndLabels(board.boardId);
			}
		}
	});
</script>

{#snippet labelChip(boardId: string, labelId: string, onRemove: () => void)}
	{@const lbl = getLabelById(boardId, labelId)}
	{#if lbl}
		<span
			class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] text-white"
			style="background-color: {trelloLabelColor(lbl.color)}"
		>
			{lbl.name || lbl.color}
			<button type="button" class="opacity-70 hover:opacity-100" onclick={onRemove}>
				<XIcon class="size-2.5" />
			</button>
		</span>
	{/if}
{/snippet}

{#snippet labelSelect(
	boardId: string,
	field: 'linkAction' | 'mergeAction',
	action: MergeAction | undefined,
	mode: 'add' | 'remove'
)}
	{@const selectedIds =
		mode === 'add' ? (action?.addLabelIds ?? []) : (action?.removeLabelIds ?? [])}
	{@const available = (labelsByBoard[boardId] ?? []).filter((l) => !selectedIds.includes(l.id))}
	<div>
		<p class="mb-0.5 text-[10px] text-muted-foreground">
			{mode === 'add' ? 'Add labels' : 'Remove labels'}
		</p>
		{#if selectedIds.length > 0}
			<div class="mb-1.5 flex flex-wrap gap-1">
				{#each selectedIds as id (id)}
					{@render labelChip(
						boardId,
						id,
						mode === 'add'
							? () => removeFromAddLabels(boardId, field, action, id)
							: () => removeFromRemoveLabels(boardId, field, action, id)
					)}
				{/each}
			</div>
		{/if}
		{#if available.length > 0}
			<select
				class="w-full rounded-md border border-border/60 bg-background px-2 py-1 text-xs"
				value=""
				onchange={(e) => {
					const val = (e.target as HTMLSelectElement).value;
					if (mode === 'add') addActionLabel(boardId, field, action, val);
					else addRemoveLabel(boardId, field, action, val);
					(e.target as HTMLSelectElement).value = '';
				}}
			>
				<option value="" disabled selected>Select a label...</option>
				{#each available as lbl (lbl.id)}
					<option value={lbl.id}>{lbl.name || lbl.color}</option>
				{/each}
			</select>
		{/if}
	</div>
{/snippet}

{#snippet actionConfig(
	boardId: string,
	label: string,
	field: 'linkAction' | 'mergeAction',
	action: MergeAction | undefined
)}
	<div>
		<p class="mb-1.5 text-[10px] font-medium text-muted-foreground">{label}</p>
		<div class="space-y-2">
			<!-- Move to column -->
			<div>
				<label class="mb-0.5 block text-[10px] text-muted-foreground" for="{field}-col-{boardId}">
					Move card to column
				</label>
				<select
					id="{field}-col-{boardId}"
					class="w-full rounded-md border border-border/60 bg-background px-2 py-1 text-xs"
					value={action?.moveToColumnId ?? ''}
					onchange={(e) =>
						setActionColumn(boardId, field, action, (e.target as HTMLSelectElement).value)}
				>
					<option value="">None</option>
					{#each columnsByBoard[boardId] ?? [] as col (col.id)}
						<option value={col.id}>{col.name}</option>
					{/each}
				</select>
			</div>

			<!-- Labels -->
			{#if (labelsByBoard[boardId]?.length ?? 0) > 0}
				{@render labelSelect(boardId, field, action, 'add')}
				{@render labelSelect(boardId, field, action, 'remove')}
			{/if}
		</div>
	</div>
{/snippet}

<div class="space-y-4">
	<div class="flex items-center justify-between">
		<h4 class="text-xs font-medium">Boards</h4>
		<Button
			variant="outline"
			size="sm"
			class="h-6 gap-1 text-[10px]"
			onclick={handleShowPicker}
			disabled={!trelloStore.authenticated}
		>
			<PlusIcon class="size-3" />
			Add Board
		</Button>
	</div>

	<!-- Board picker -->
	{#if showPicker}
		<div class="rounded-md border border-border/60 p-2">
			{#if loadingPicker}
				<div class="flex items-center justify-center py-4">
					<LoaderIcon class="size-4 animate-spin text-muted-foreground" />
				</div>
			{:else if pickableBoards.length === 0}
				<p class="py-2 text-center text-[10px] text-muted-foreground">
					All available boards have been added
				</p>
				<Button
					variant="ghost"
					size="sm"
					class="mt-1 h-6 text-[10px]"
					onclick={() => (showPicker = false)}
				>
					Close
				</Button>
			{:else}
				<p class="mb-2 text-[10px] font-medium text-muted-foreground">Select a board:</p>
				<div class="max-h-40 space-y-0.5 overflow-y-auto">
					{#each pickableBoards as board (board.id)}
						<button
							type="button"
							class="w-full rounded px-2 py-1 text-left text-xs hover:bg-accent/50"
							onclick={() => handleAddBoard(board.id, board.name)}
						>
							{board.name}
						</button>
					{/each}
				</div>
				<Button
					variant="ghost"
					size="sm"
					class="mt-2 h-6 text-[10px]"
					onclick={() => (showPicker = false)}
				>
					Cancel
				</Button>
			{/if}
		</div>
	{/if}

	<!-- Configured boards -->
	{#each boards as board (board.boardId)}
		<div class="rounded-md border border-border/60 p-3">
			<div class="flex items-center justify-between">
				<span class="text-xs font-medium">{board.boardName}</span>
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-6 text-destructive"
					onclick={() => handleRemoveBoard(board.boardId)}
				>
					<TrashIcon class="size-3" />
				</Button>
			</div>

			<!-- Column visibility -->
			{#if columnsByBoard[board.boardId]}
				<div class="mt-2">
					<p class="mb-1 text-[10px] font-medium text-muted-foreground">Visible Columns</p>
					<div class="space-y-0.5">
						{#each columnsByBoard[board.boardId] as col (col.id)}
							<label class="flex items-center gap-2 text-xs">
								<Checkbox
									checked={!board.hiddenColumns.includes(col.id)}
									onCheckedChange={() => toggleColumn(board.boardId, col.id)}
								/>
								{col.name}
							</label>
						{/each}
					</div>
				</div>
			{/if}

			<Separator class="my-2" />
			{@render actionConfig(board.boardId, 'On Task Link', 'linkAction', board.linkAction)}

			<Separator class="my-2" />
			{@render actionConfig(board.boardId, 'On PR Merge', 'mergeAction', board.mergeAction)}
		</div>
	{/each}

	{#if boards.length === 0 && !showPicker}
		<p class="text-xs text-muted-foreground">No boards configured for this project</p>
	{/if}
</div>
