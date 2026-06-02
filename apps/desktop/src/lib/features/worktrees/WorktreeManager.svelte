<script lang="ts">
	import ConfirmDialog from '$components/ConfirmDialog.svelte';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import WorktreeDialog from './WorktreeDialog.svelte';
	import { getWorktreeManager } from '$stores/context';

	const manager = getWorktreeManager();

	let removalBranch = $derived(manager.removal.pendingValue?.branch ?? '');
</script>

<WorktreeDialog
	bind:open={manager.dialogOpen}
	branches={manager.dialogBranches}
	projectPath={manager.dialogProjectPath}
	error={manager.dialogError}
	suggestedBranch={manager._suggestedBranch}
	onSave={(branch, newBranch, path, copyOptions) =>
		manager.create(branch, newBranch, path, copyOptions)}
/>

<ConfirmDialog
	bind:open={manager.removal.open}
	title="Remove Worktree"
	description="Remove this git worktree from disk and close its workspace?"
	confirmLabel={manager.removal.error ? 'Force Remove' : 'Remove'}
	error={manager.removal.error}
	destructive
	onConfirm={() => manager.confirmRemove(!!manager.removal.error)}
>
	{#if removalBranch}
		<label class="flex items-center gap-2 text-sm">
			<Checkbox
				checked={manager.deleteBranchOnRemove}
				onCheckedChange={(v) => (manager.deleteBranchOnRemove = v === true)}
			/>
			<span class="text-muted-foreground">
				Also delete branch <span class="font-medium text-foreground">{removalBranch}</span>
			</span>
		</label>
	{/if}
</ConfirmDialog>
