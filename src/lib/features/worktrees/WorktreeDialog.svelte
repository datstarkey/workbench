<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Select from '$lib/components/ui/select';
	import type { BranchInfo } from '$types/workbench';
	import { baseName } from '$lib/utils/path';

	let {
		open = $bindable(),
		branches,
		projectPath,
		error,
		onSave
	}: {
		open: boolean;
		branches: BranchInfo[];
		projectPath: string;
		error: string;
		onSave: (branch: string, newBranch: boolean, path: string) => void;
	} = $props();

	let mode: 'new' | 'existing' = $state('new');
	let newBranchName: string = $state('');
	let selectedBranch: string = $state('');

	let branchName = $derived(mode === 'new' ? newBranchName : selectedBranch);

	let parentDir = $derived(projectPath.replace(/\/?$/, '').replace(/\/[^/]+$/, ''));
	let projectDirName = $derived(baseName(projectPath));
	let worktreePath = $derived(branchName ? `${parentDir}/${projectDirName}-${branchName}` : '');

	let localBranches = $derived(branches.filter((b) => !b.isRemote && !b.isCurrent));

	function handleSave() {
		if (!branchName) return;
		onSave(branchName, mode === 'new', worktreePath);
	}

	function resetState() {
		mode = 'new';
		newBranchName = '';
		selectedBranch = '';
	}
</script>

<Dialog.Root bind:open onOpenChange={(isOpen) => !isOpen && resetState()}>
	<Dialog.Content class="sm:max-w-lg">
		<Dialog.Header>
			<Dialog.Title>Create Worktree</Dialog.Title>
			<Dialog.Description>Create a new git worktree for parallel development.</Dialog.Description>
		</Dialog.Header>

		<div class="grid gap-4 py-4">
			<div class="flex gap-2">
				<Button
					variant={mode === 'new' ? 'default' : 'outline'}
					size="sm"
					onclick={() => (mode = 'new')}
				>
					New branch
				</Button>
				<Button
					variant={mode === 'existing' ? 'default' : 'outline'}
					size="sm"
					onclick={() => (mode = 'existing')}
				>
					Existing branch
				</Button>
			</div>

			{#if mode === 'new'}
				<div class="grid gap-2">
					<Label for="branch-name">Branch name</Label>
					<Input id="branch-name" placeholder="feature/my-branch" bind:value={newBranchName} />
				</div>
			{:else}
				<div class="grid gap-2">
					<Label>Branch</Label>
					<Select.Root
						type="single"
						value={selectedBranch}
						onValueChange={(v) => (selectedBranch = v)}
					>
						<Select.Trigger class="w-full">
							{selectedBranch || 'Select a branch'}
						</Select.Trigger>
						<Select.Content>
							{#each localBranches as branch (branch.name)}
								<Select.Item value={branch.name}>{branch.name}</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
				</div>
			{/if}

			{#if worktreePath}
				<div class="grid gap-2">
					<Label>Worktree path</Label>
					<Input value={worktreePath} readonly class="text-muted-foreground" />
				</div>
			{/if}

			{#if error}
				<p class="text-sm text-destructive">{error}</p>
			{/if}
		</div>

		<Dialog.Footer>
			<Button variant="ghost" onclick={() => (open = false)}>Cancel</Button>
			<Button onclick={handleSave} disabled={!branchName}>Create Worktree</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
