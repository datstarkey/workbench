<script lang="ts">
	import { Label } from '$lib/components/ui/label';
	import * as Select from '$lib/components/ui/select';
	import { claudeSettingsStore } from '$stores/claude-settings.svelte';
	import type { PermissionsConfig } from '$types/claude-settings';
	import EditableStringList from './EditableStringList.svelte';
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
	<div>
		<h3 class="text-sm font-medium">Default Permission Mode</h3>
		<p class="mt-1 text-xs text-muted-foreground">
			Controls how Claude handles tool permissions by default.
		</p>
		<div class="mt-2">
			<Select.Root
				type="single"
				value={perms.defaultMode ?? 'default'}
				onValueChange={(v) => updatePerms({ defaultMode: v })}
			>
				<Select.Trigger class="w-48">
					{modeOptions.find((o) => o.value === (perms.defaultMode ?? 'default'))?.label ??
						'Default'}
				</Select.Trigger>
				<Select.Content>
					{#each modeOptions as opt (opt.value)}
						<Select.Item value={opt.value}>{opt.label}</Select.Item>
					{/each}
				</Select.Content>
			</Select.Root>
		</div>
	</div>

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
				const dir = v.trim();
				if (!dir) return;
				const current = [...additionalDirs];
				if (!current.includes(dir)) {
					current.push(dir);
					updatePerms({ additionalDirectories: current });
				}
			}}
			onRemove={(dir) => {
				updatePerms({ additionalDirectories: additionalDirs.filter((d) => d !== dir) });
			}}
			placeholder="/path/to/directory"
		/>
	</div>
</div>
