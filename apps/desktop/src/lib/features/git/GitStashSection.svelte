<script lang="ts">
	import ArchiveIcon from '@lucide/svelte/icons/archive';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import TrashIcon from '@lucide/svelte/icons/trash-2';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { getGitStore } from '$stores/context';
	import type { GitStashEntry } from '$types/workbench';
	import { toast } from 'svelte-sonner';

	let {
		stashes,
		path
	}: {
		stashes: GitStashEntry[];
		path: string;
	} = $props();

	const gitStore = getGitStore();

	let expanded = $state(true);
	let stashMessage = $state('');
	let showInput = $state(false);

	async function handlePush() {
		try {
			await gitStore.stashPush(path, stashMessage.trim() || undefined);
			stashMessage = '';
			showInput = false;
			toast.success('Stash saved');
		} catch (e) {
			toast.error(`Failed to stash: ${e}`);
		}
	}

	async function handlePop(index: number) {
		try {
			await gitStore.stashPop(path, index);
			toast.success('Stash applied');
		} catch (e) {
			toast.error(`Failed to pop stash: ${e}`);
		}
	}

	async function handleDrop(index: number) {
		try {
			await gitStore.stashDrop(path, index);
			toast.success('Stash dropped');
		} catch (e) {
			toast.error(`Failed to drop stash: ${e}`);
		}
	}
</script>

<div>
	<div class="flex items-center">
		<button
			type="button"
			class="flex flex-1 items-center gap-1 px-2 py-1 text-xs font-medium text-muted-foreground hover:text-foreground"
			onclick={() => (expanded = !expanded)}
		>
			{#if expanded}
				<ChevronDownIcon class="size-3" />
			{:else}
				<ChevronRightIcon class="size-3" />
			{/if}
			Stashes
		</button>
		<Button
			variant="ghost"
			size="icon-sm"
			class="mr-1 size-5"
			onclick={() => (showInput = !showInput)}
			title="Stash changes"
		>
			<ArchiveIcon class="size-3" />
		</Button>
	</div>
	{#if expanded}
		{#if showInput}
			<div class="flex items-center gap-1 px-2 pb-1">
				<Input
					bind:value={stashMessage}
					placeholder="Stash message (optional)..."
					class="h-7 text-xs"
					onkeydown={(e) => {
						if (e.key === 'Enter') {
							e.preventDefault();
							handlePush();
						}
					}}
				/>
				<Button variant="secondary" size="sm" class="h-7 shrink-0 text-xs" onclick={handlePush}>
					Stash
				</Button>
			</div>
		{/if}
		<div class="space-y-0.5 px-1">
			{#each stashes as stash (stash.index)}
				<div class="group flex items-center gap-1.5 rounded px-1.5 py-0.5 hover:bg-muted/40">
					<span class="min-w-0 flex-1 truncate text-xs text-foreground/80" title={stash.message}
						>{stash.message || `stash@{${stash.index}}`}</span
					>
					<div class="flex shrink-0 items-center gap-0.5 opacity-0 group-hover:opacity-100">
						<Button
							variant="ghost"
							size="icon-sm"
							class="size-5"
							onclick={() => handlePop(stash.index)}
							title="Pop stash"
						>
							<UploadIcon class="size-3" />
						</Button>
						<Button
							variant="ghost"
							size="icon-sm"
							class="size-5"
							onclick={() => handleDrop(stash.index)}
							title="Drop stash"
						>
							<TrashIcon class="size-3" />
						</Button>
					</div>
				</div>
			{/each}
			{#if stashes.length === 0}
				<p class="px-1.5 py-1 text-xs text-muted-foreground">No stashes</p>
			{/if}
		</div>
	{/if}
</div>
