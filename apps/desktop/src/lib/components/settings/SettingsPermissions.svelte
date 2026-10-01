<script lang="ts">
	import { getClaudeSettingsStore } from '$stores/context';
	import type { PermissionsConfig } from '$types/claude-settings';
	import EditableStringList from './EditableStringList.svelte';
	import SettingsRow from './SettingsRow.svelte';
	import SettingsSection from './SettingsSection.svelte';
	import SettingsSelect from './SettingsSelect.svelte';
	import SettingsToggle from './SettingsToggle.svelte';

	const claudeSettingsStore = getClaudeSettingsStore();

	let settings = $derived(claudeSettingsStore.currentSettings);
	let perms = $derived((settings.permissions ?? {}) as PermissionsConfig);
	let additionalDirs = $derived(perms.additionalDirectories ?? []);

	const rules = [
		{
			kind: 'allow',
			label: 'Allow',
			description: 'Run without asking.',
			placeholder: 'e.g. Bash(git *)',
			badgeVariant: 'secondary'
		},
		{
			kind: 'ask',
			label: 'Ask',
			description: 'Always confirm first.',
			placeholder: 'e.g. Bash(gh pr create)',
			badgeVariant: 'outline'
		},
		{
			kind: 'deny',
			label: 'Deny',
			description: 'Never allowed.',
			placeholder: 'e.g. Bash(rm *)',
			badgeVariant: 'destructive'
		}
	] as const;

	const modeOptions = [
		{ value: 'default', label: 'Default' },
		{ value: 'acceptEdits', label: 'Accept edits' },
		{ value: 'plan', label: 'Plan' },
		{ value: 'dontAsk', label: "Don't ask" },
		{ value: 'bypassPermissions', label: 'Bypass permissions' }
	];

	function updatePerms(partial: Partial<PermissionsConfig>) {
		claudeSettingsStore.updateNested('permissions', { ...perms, ...partial });
	}
</script>

<SettingsSection title="Rules">
	{#each rules as rule (rule.kind)}
		<SettingsRow label={rule.label} description={rule.description} stack>
			{#snippet control()}
				<EditableStringList
					items={perms[rule.kind] ?? []}
					onAdd={(v) => claudeSettingsStore.addPermission(rule.kind, v)}
					onRemove={(v) => claudeSettingsStore.removePermission(rule.kind, v)}
					placeholder={rule.placeholder}
					badgeVariant={rule.badgeVariant}
				/>
			{/snippet}
		</SettingsRow>
	{/each}
</SettingsSection>

<SettingsSection title="Defaults">
	<SettingsSelect
		label="Default mode"
		description="Sessions Workbench starts use Claude Code › Sessions › Permission mode instead."
		options={modeOptions}
		value={perms.defaultMode ?? 'default'}
		onValueChange={(v) => updatePerms({ defaultMode: v as typeof perms.defaultMode })}
	/>

	<SettingsToggle
		label="Disable bypass mode"
		description="Stop anyone switching to Bypass permissions."
		checked={perms.disableBypassPermissionsMode === 'disable'}
		onCheckedChange={(v) =>
			updatePerms({ disableBypassPermissionsMode: v ? 'disable' : undefined })}
	/>

	<SettingsRow
		label="Additional directories"
		description="Folders Claude may use beyond the project root."
		stack
	>
		{#snippet control()}
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
		{/snippet}
	</SettingsRow>
</SettingsSection>
