<script lang="ts">
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import * as Select from '$lib/components/ui/select';
	import { getTrelloStore, getGitStore } from '$stores/context';
	import type { TrelloCard } from '$types/trello';

	let {
		open = $bindable(false),
		card,
		projectPath,
		boardId
	}: {
		open: boolean;
		card: TrelloCard | null;
		projectPath: string;
		boardId: string;
	} = $props();

	const trelloStore = getTrelloStore();
	const gitStore = getGitStore();

	let selectedBranch = $state('');
	let branches = $derived(Object.keys(gitStore.branchByProject));

	function handleLink() {
		if (!card || !selectedBranch) return;
		trelloStore.linkTaskToBranch(projectPath, card.id, boardId, selectedBranch);
		open = false;
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-w-sm">
		<Dialog.Header>
			<Dialog.Title>Link Task to Branch</Dialog.Title>
			<Dialog.Description>
				{#if card}
					Link "{card.name}" to a branch
				{/if}
			</Dialog.Description>
		</Dialog.Header>

		<div class="space-y-3 py-2">
			<Select.Root type="single" bind:value={selectedBranch}>
				<Select.Trigger class="w-full">
					{selectedBranch || 'Select a branch...'}
				</Select.Trigger>
				<Select.Content>
					{#each branches as branch (branch)}
						<Select.Item value={branch}>{branch}</Select.Item>
					{/each}
				</Select.Content>
			</Select.Root>
		</div>

		<Dialog.Footer>
			<Button variant="outline" onclick={() => (open = false)}>Cancel</Button>
			<Button onclick={handleLink} disabled={!selectedBranch}>Link</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
