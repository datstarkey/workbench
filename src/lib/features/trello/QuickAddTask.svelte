<script lang="ts">
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { getTrelloStore } from '$stores/context';

	let { listId, projectPath }: { listId: string; projectPath: string } = $props();

	const trelloStore = getTrelloStore();

	let expanded = $state(false);
	let title = $state('');
	let submitting = $state(false);

	async function handleAdd() {
		if (!title.trim()) return;
		submitting = true;
		try {
			await trelloStore.createCard(projectPath, listId, title.trim());
			title = '';
			expanded = false;
		} finally {
			submitting = false;
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') handleAdd();
		if (e.key === 'Escape') {
			expanded = false;
			title = '';
		}
	}
</script>

{#if expanded}
	<div class="px-2 py-1">
		<Input
			class="h-7 text-xs"
			placeholder="Card title..."
			bind:value={title}
			onkeydown={handleKeydown}
			disabled={submitting}
		/>
		<div class="mt-1.5 flex gap-1">
			<Button
				size="sm"
				class="h-6 text-[10px]"
				onclick={handleAdd}
				disabled={submitting || !title.trim()}
			>
				Add
			</Button>
			<Button
				variant="ghost"
				size="sm"
				class="h-6 text-[10px]"
				onclick={() => {
					expanded = false;
					title = '';
				}}
			>
				Cancel
			</Button>
		</div>
	</div>
{:else}
	<Button
		variant="ghost"
		size="sm"
		class="h-6 w-full justify-start gap-1 px-2 text-[10px] text-muted-foreground"
		onclick={() => (expanded = true)}
	>
		<PlusIcon class="size-3" />
		Add task
	</Button>
{/if}
