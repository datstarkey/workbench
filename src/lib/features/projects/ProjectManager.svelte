<script lang="ts">
	import ConfirmDialog from '$components/ConfirmDialog.svelte';
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
/>

<ConfirmDialog
	bind:open={manager.removal.open}
	title="Remove Project"
	description="Remove this project from Workbench? This won't delete the folder."
	confirmLabel="Remove"
	destructive
	onConfirm={() => manager.confirmRemove()}
/>
