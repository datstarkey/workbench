<script lang="ts">
	import { watch } from 'runed';
	import { getTrelloStore } from '$stores/context';
	import { Separator } from '@workbench/ui/separator';
	import BoardEmptyState from './BoardEmptyState.svelte';
	import BoardSection from './BoardSection.svelte';

	const trelloStore = getTrelloStore();

	let projectPath = $derived(trelloStore.activeProjectPath);
	let boards = $derived(trelloStore.activeBoards);
	let boardDataList = $derived(trelloStore.activeBoardData);

	// Fetch board data when active project changes (network side effect)
	watch(
		() => [projectPath, boards],
		() => {
			if (projectPath && boards.length > 0) {
				trelloStore.refreshAllBoards(projectPath);
			}
		}
	);
</script>

{#if boards.length === 0}
	<BoardEmptyState />
{:else}
	<div class="p-2">
		{#each boardDataList as boardData, i (boardData.board.id)}
			{#if i > 0}
				<Separator class="my-2" />
			{/if}
			<BoardSection {boardData} {projectPath} />
		{/each}
	</div>
{/if}
