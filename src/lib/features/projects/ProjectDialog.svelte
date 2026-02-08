<script lang="ts">
	import PlusIcon from '@lucide/svelte/icons/plus';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import type { ProjectFormState } from '$types/workbench';

	let {
		open = $bindable(),
		mode,
		form = $bindable(),
		error,
		onSave,
		onPickFolder,
		onAddTask,
		onRemoveTask,
		onUpdateTaskName,
		onUpdateTaskCommand
	}: {
		open: boolean;
		mode: 'create' | 'edit';
		form: ProjectFormState;
		error: string;
		onSave: () => void;
		onPickFolder: () => void;
		onAddTask: () => void;
		onRemoveTask: (index: number) => void;
		onUpdateTaskName: (index: number, name: string) => void;
		onUpdateTaskCommand: (index: number, command: string) => void;
	} = $props();
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-lg">
		<Dialog.Header>
			<Dialog.Title>{mode === 'create' ? 'Add Project' : 'Edit Project'}</Dialog.Title>
			<Dialog.Description>
				Point Workbench at a local folder. Terminals will open in this directory.
			</Dialog.Description>
		</Dialog.Header>

		<div class="grid gap-4 py-2">
			<div class="grid gap-1.5">
				<label class="text-sm font-medium" for="project-name">Name</label>
				<Input id="project-name" bind:value={form.name} placeholder="Client Site" />
			</div>

			<div class="grid gap-1.5">
				<label class="text-sm font-medium" for="project-path">Folder</label>
				<div class="flex gap-2">
					<Input id="project-path" bind:value={form.path} placeholder="/code/client-site" />
					<Button type="button" variant="outline" onclick={onPickFolder}>Browse</Button>
				</div>
			</div>

			<div class="grid gap-1.5">
				<label class="text-sm font-medium" for="project-shell"
					>Shell <span class="font-normal text-muted-foreground">(optional)</span></label
				>
				<Input id="project-shell" bind:value={form.shell} placeholder="/bin/zsh" />
			</div>

			<div class="grid gap-1.5">
				<label class="text-sm font-medium" for="project-startup"
					>Startup command <span class="font-normal text-muted-foreground">(optional)</span></label
				>
				<Input id="project-startup" bind:value={form.startupCommand} placeholder="bun dev" />
			</div>

			<div class="grid gap-2">
				<div class="flex items-center justify-between">
					<p class="text-sm font-medium">Project tasks</p>
					<Button type="button" variant="outline" size="sm" onclick={onAddTask}>
						<PlusIcon class="size-3.5" />
						Add Task
					</Button>
				</div>
				<p class="text-xs text-muted-foreground">
					Tasks run in a new terminal tab inside this project workspace.
				</p>
				{#if form.tasks.length === 0}
					<p class="text-xs text-muted-foreground/70">No tasks yet.</p>
				{:else}
					<div class="space-y-2">
						{#each form.tasks as task, i (i)}
							<div class="grid gap-2 rounded-md border border-border/60 p-2">
								<div class="flex items-center gap-2">
									<Input
										value={task.name}
										placeholder="Task name (e.g. Tests)"
										oninput={(event) =>
											onUpdateTaskName(i, (event.currentTarget as HTMLInputElement).value)}
									/>
									<Button
										type="button"
										variant="ghost"
										size="icon-sm"
										class="size-8 shrink-0 text-muted-foreground hover:text-destructive"
										onclick={() => onRemoveTask(i)}
										aria-label="Remove task"
									>
										<XIcon class="size-3.5" />
									</Button>
								</div>
								<Input
									value={task.command}
									placeholder="dotnet test"
									oninput={(event) =>
										onUpdateTaskCommand(i, (event.currentTarget as HTMLInputElement).value)}
								/>
							</div>
						{/each}
					</div>
				{/if}
			</div>

			{#if error}
				<p class="text-sm text-destructive">{error}</p>
			{/if}
		</div>

		<Dialog.Footer>
			<Button type="button" variant="ghost" onclick={() => (open = false)}>Cancel</Button>
			<Button type="button" onclick={onSave}
				>{mode === 'create' ? 'Create Project' : 'Save Changes'}</Button
			>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
