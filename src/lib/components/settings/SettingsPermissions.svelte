<script lang="ts">
	import { Label } from '$lib/components/ui/label';
	import { getClaudeSettingsStore } from '$stores/context';
	import type { PermissionsConfig } from '$types/claude-settings';

	const claudeSettingsStore = getClaudeSettingsStore();
	import EditableStringList from './EditableStringList.svelte';
	import SettingsSelect from './SettingsSelect.svelte';
	import SettingsToggle from './SettingsToggle.svelte';

	let settings = $derived(claudeSettingsStore.currentSettings);
	let perms = $derived((settings.permissions ?? {}) as PermissionsConfig);
	let allowList = $derived(perms.allow ?? []);
	let denyList = $derived(perms.deny ?? []);
	let askList = $derived(perms.ask ?? []);
	let additionalDirs = $derived(perms.additionalDirectories ?? []);

	const modeOptions = [
		{ value: 'default', label: 'Default' },
		{ value: 'acceptEdits', label: 'Accept Edits' },
		{ value: 'plan', label: 'Plan' },
		{ value: 'dontAsk', label: "Don't Ask" },
		{ value: 'bypassPermissions', label: 'Bypass Permissions' }
	];

	function updatePerms(partial: Partial<PermissionsConfig>) {
		claudeSettingsStore.updateNested('permissions', { ...perms, ...partial });
	}
</script>

<div class="space-y-6">
	<SettingsSelect
		label="Default Permission Mode"
		description="Controls how Claude handles tool permissions by default."
		options={modeOptions}
		value={perms.defaultMode ?? 'default'}
		onValueChange={(v) => updatePerms({ defaultMode: v as typeof perms.defaultMode })}
		triggerClass="w-48"
	/>

	<SettingsToggle
		label="Disable Bypass Mode"
		description="Prevent users from enabling bypass permissions mode."
		checked={perms.disableBypassPermissionsMode === 'disable'}
		onCheckedChange={(v) =>
			updatePerms({ disableBypassPermissionsMode: v ? 'disable' : undefined })}
	/>

	<div>
		<Label class="text-sm font-medium">Allow List</Label>
		<p class="mt-1 text-xs text-muted-foreground">Tool patterns that are automatically allowed.</p>
		<EditableStringList
			items={allowList}
			onAdd={(v) => claudeSettingsStore.addPermission('allow', v)}
			onRemove={(v) => claudeSettingsStore.removePermission('allow', v)}
			placeholder="e.g. Bash(git *)"
			badgeVariant="secondary"
		/>
	</div>

	<div>
		<Label class="text-sm font-medium">Deny List</Label>
		<p class="mt-1 text-xs text-muted-foreground">Tool patterns that are always denied.</p>
		<EditableStringList
			items={denyList}
			onAdd={(v) => claudeSettingsStore.addPermission('deny', v)}
			onRemove={(v) => claudeSettingsStore.removePermission('deny', v)}
			placeholder="e.g. Bash(rm *)"
			badgeVariant="destructive"
		/>
	</div>

	<div>
		<Label class="text-sm font-medium">Ask List</Label>
		<p class="mt-1 text-xs text-muted-foreground">
			Tool patterns that always require user confirmation.
		</p>
		<EditableStringList
			items={askList}
			onAdd={(v) => claudeSettingsStore.addPermission('ask', v)}
			onRemove={(v) => claudeSettingsStore.removePermission('ask', v)}
			placeholder="e.g. Bash(gh pr create)"
			badgeVariant="outline"
		/>
	</div>

	<div>
		<Label class="text-sm font-medium">Additional Directories</Label>
		<p class="mt-1 text-xs text-muted-foreground">
			Extra directories Claude is allowed to access beyond the project root.
		</p>
		<EditableStringList
			items={additionalDirs}
			onAdd={(v) => {
				if (!additionalDirs.includes(v)) {
					updatePerms({ additionalDirectories: [...additionalDirs, v] });
				}
			}}
			onRemove={(v) => {
				updatePerms({ additionalDirectories: additionalDirs.filter((d) => d !== v) });
			}}
			placeholder="/path/to/directory"
		/>
	</div>
</div>
