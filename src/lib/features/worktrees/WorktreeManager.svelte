<script lang="ts">
	import ConfirmDialog from '$components/ConfirmDialog.svelte';
	import WorktreeDialog from './WorktreeDialog.svelte';
	import { getWorktreeManager } from '$stores/context';

	const manager = getWorktreeManager();
</script>

<WorktreeDialog
	bind:open={manager.dialogOpen}
	branches={manager.dialogBranches}
	projectPath={manager.dialogProjectPath}
	error={manager.dialogError}
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
/>
