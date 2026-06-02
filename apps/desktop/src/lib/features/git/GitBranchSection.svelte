<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { getGitStore } from '$stores/context';
	import type { BranchInfo } from '$types/workbench';
	import { invoke } from '@tauri-apps/api/core';
	import { toast } from 'svelte-sonner';

	let { path }: { path: string } = $props();

	const gitStore = getGitStore();

	let branches: BranchInfo[] = $state([]);
	let expanded = $state(true);
	let showInput = $state(false);
	let newBranchName = $state('');

	$effect(() => {
		void path;
		loadBranches();
	});

	async function loadBranches() {
		try {
			branches = await invoke<BranchInfo[]>('list_branches', { path });
		} catch (e) {
			console.warn('[GitBranchSection] Failed to load branches:', e);
		}
	}

	let localBranches = $derived(branches.filter((b) => !b.isRemote));

	async function checkout(branch: string) {
		try {
			await gitStore.checkoutBranch(path, branch);
			await loadBranches();
			toast.success(`Switched to ${branch}`);
		} catch (e) {
			toast.error(`Failed to checkout: ${e}`);
		}
	}

	async function handleCreate() {
		const name = newBranchName.trim();
		if (!name) return;
		try {
			await gitStore.createBranch(path, name, true);
			newBranchName = '';
			showInput = false;
			await loadBranches();
			toast.success(`Created and switched to ${name}`);
		} catch (e) {
			toast.error(`Failed to create branch: ${e}`);
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
			Branches
		</button>
		<Button
			variant="ghost"
			size="icon-sm"
			class="mr-1 size-5"
			onclick={() => (showInput = !showInput)}
			title="Create branch"
		>
			<PlusIcon class="size-3" />
		</Button>
	</div>
	{#if expanded}
		{#if showInput}
			<div class="flex items-center gap-1 px-2 pb-1">
				<Input
					bind:value={newBranchName}
					placeholder="Branch name..."
					class="h-7 text-xs"
					onkeydown={(e: KeyboardEvent) => {
						if (e.key === 'Enter') {
							e.preventDefault();
							handleCreate();
						}
					}}
				/>
				<Button
					variant="secondary"
					size="sm"
					class="h-7 shrink-0 text-xs"
					disabled={!newBranchName.trim()}
					onclick={handleCreate}
				>
					Create
				</Button>
			</div>
		{/if}
		<div class="space-y-0.5 px-1">
			{#each localBranches as branch (branch.name)}
				<Button
					variant="ghost"
					size="sm"
					class="h-6 w-full justify-start gap-1.5 px-1.5 text-xs font-normal {branch.isCurrent
						? 'text-foreground'
						: 'text-muted-foreground'}"
					disabled={branch.isCurrent}
					onclick={() => checkout(branch.name)}
				>
					{#if branch.isCurrent}
						<CheckIcon class="size-3 shrink-0 text-green-400" />
					{:else}
						<span class="size-3 shrink-0"></span>
					{/if}
					<span class="truncate">{branch.name}</span>
				</Button>
			{/each}
			{#if localBranches.length === 0}
				<p class="px-1.5 py-1 text-xs text-muted-foreground">No branches found</p>
			{/if}
		</div>
	{/if}
</div>
