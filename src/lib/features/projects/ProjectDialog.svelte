<script lang="ts">
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
		onPickFolder
	}: {
		open: boolean;
		mode: 'create' | 'edit';
		form: ProjectFormState;
		error: string;
		onSave: () => void;
		onPickFolder: () => void;
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
