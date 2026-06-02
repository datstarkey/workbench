<script lang="ts">
	import ConfirmDialog from '$components/ConfirmDialog.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import ProjectDialog from './ProjectDialog.svelte';
	import { getProjectManager } from '$stores/context';

	const manager = getProjectManager();
</script>

<ProjectDialog
	bind:open={manager.dialogOpen}
	mode={manager.dialogMode}
	bind:form={manager.form}
	error={manager.formError}
	onSave={() => manager.save()}
	onPickFolder={() => manager.pickFolder()}
	onAddTask={() => manager.addTask()}
	onRemoveTask={(index) => manager.removeTask(index)}
	onUpdateTaskName={(index, name) => manager.updateTaskName(index, name)}
	onUpdateTaskCommand={(index, command) => manager.updateTaskCommand(index, command)}
	onReorderTask={(fromIndex, toIndex) => manager.reorderTask(fromIndex, toIndex)}
/>

<ConfirmDialog
	bind:open={manager.removal.open}
	title="Remove Project"
	description="Remove this project from Workbench? This won't delete the folder."
	confirmLabel="Remove"
	destructive
	onConfirm={() => manager.confirmRemove()}
/>

<Dialog.Root bind:open={manager.groupDialogOpen}>
	<Dialog.Content class="sm:max-w-sm">
		<Dialog.Header>
			<Dialog.Title>New Group</Dialog.Title>
			<Dialog.Description>Enter a name for the project group.</Dialog.Description>
		</Dialog.Header>
		<div class="py-2">
			<Input
				bind:value={manager.groupDialogValue}
				placeholder="e.g. Work, Personal"
				onkeydown={(e: KeyboardEvent) => {
					if (e.key === 'Enter') manager.saveNewGroup();
				}}
			/>
		</div>
		<Dialog.Footer>
			<Button variant="ghost" onclick={() => (manager.groupDialogOpen = false)}>Cancel</Button>
			<Button onclick={() => manager.saveNewGroup()}>Create</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
