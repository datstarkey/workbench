<script lang="ts">
	import { Button } from '@workbench/ui/button';
	import * as Dialog from '@workbench/ui/dialog';
	import { Input } from '@workbench/ui/input';
	import * as Select from '@workbench/ui/select';
	import type { ProjectFormState } from '$types/workbench';
	import { accountChoices } from '@workbench/chat-ui';
	import { getWorkbenchSettingsStore } from '$stores/context';

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

	const settings = getWorkbenchSettingsStore();
	/** Follows the active account; the other options are `accountChoices` keys. */
	const ACTIVE = 'active';
	const choices = $derived(accountChoices(settings.claudeAccounts, settings.defaultAccountName));
	const accountOptions = $derived([
		{ key: ACTIVE, label: 'Active account' },
		...choices.map((c) => ({ key: c.key, label: c.id ? c.name : `${c.name} (~/.claude)` }))
	]);
	// `''` saves the default login; a removed account's id behaves as (and shows) the active one.
	const accountKey = $derived(
		form.claudeAccountId === ''
			? (choices.find((c) => !c.id)?.key ?? ACTIVE)
			: (choices.find((c) => c.id && c.id === form.claudeAccountId)?.key ?? ACTIVE)
	);
	const accountLabel = $derived(accountOptions.find((o) => o.key === accountKey)?.label);

	function setAccount(key: string) {
		const choice = choices.find((c) => c.key === key);
		form = { ...form, claudeAccountId: key === ACTIVE ? undefined : (choice?.id ?? '') };
	}
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
				<label class="text-sm font-medium" for="project-group"
					>Group <span class="font-normal text-muted-foreground">(optional)</span></label
				>
				<Input id="project-group" bind:value={form.group} placeholder="e.g. Work, Personal" />
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

			{#if settings.claudeAccounts.length > 0}
				<div class="grid gap-1.5">
					<label class="text-sm font-medium" for="project-account">Claude account</label>
					<Select.Root type="single" value={accountKey} onValueChange={setAccount}>
						<Select.Trigger id="project-account">{accountLabel}</Select.Trigger>
						<Select.Content>
							{#each accountOptions as option (option.key)}
								<Select.Item value={option.key}>{option.label}</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
					<p class="text-xs text-muted-foreground">
						New Claude sessions in this project start under it.
					</p>
				</div>
			{/if}

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
