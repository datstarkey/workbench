<script lang="ts">
	import { Input } from '@workbench/ui/input';
	import { getClaudeSettingsStore } from '$stores/context';
	import SettingsRow from './SettingsRow.svelte';
	import SettingsSection from './SettingsSection.svelte';
	import SettingsSelect from './SettingsSelect.svelte';
	import SettingsToggle from './SettingsToggle.svelte';

	const claudeSettingsStore = getClaudeSettingsStore();

	const effortOptions = [
		{ value: 'low', label: 'Low' },
		{ value: 'medium', label: 'Medium' },
		{ value: 'high', label: 'High' },
		{ value: 'max', label: 'Max' }
	];

	const updateChannelOptions = [
		{ value: 'stable', label: 'Stable' },
		{ value: 'latest', label: 'Latest' }
	];

	const notifChannelOptions = [
		{ value: 'terminal', label: 'Terminal' },
		{ value: 'iterm2', label: 'iTerm2' },
		{ value: 'terminal_bell', label: 'Terminal bell' }
	];

	let settings = $derived(claudeSettingsStore.currentSettings);
</script>

<SettingsSection title="Responses">
	<SettingsSelect
		label="Effort level"
		description="How much effort Claude puts into responses."
		options={effortOptions}
		value={settings.effortLevel ?? 'high'}
		onValueChange={(v) =>
			claudeSettingsStore.update({ effortLevel: v as typeof settings.effortLevel })}
	/>

	<SettingsToggle
		label="Always enable thinking"
		description="Use extended thinking by default."
		checked={settings.alwaysThinkingEnabled ?? false}
		onCheckedChange={(v) => claudeSettingsStore.update({ alwaysThinkingEnabled: v })}
	/>

	<SettingsRow label="Language" description="Preferred response language.">
		{#snippet control()}
			<Input
				class="h-8 w-44 text-xs"
				placeholder="e.g. english, japanese"
				aria-label="Language"
				value={settings.language ?? ''}
				oninput={(e) => {
					const val = e.currentTarget.value;
					claudeSettingsStore.update({ language: val || undefined });
				}}
			/>
		{/snippet}
	</SettingsRow>
</SettingsSection>

<SettingsSection title="Interface">
	<SettingsToggle
		label="Show turn duration"
		description="Display how long each turn takes."
		checked={settings.showTurnDuration ?? true}
		onCheckedChange={(v) => claudeSettingsStore.update({ showTurnDuration: v })}
	/>

	<SettingsToggle
		label="Spinner tips"
		description="Show tips while Claude is working."
		checked={settings.spinnerTipsEnabled ?? true}
		onCheckedChange={(v) => claudeSettingsStore.update({ spinnerTipsEnabled: v })}
	/>

	<SettingsToggle
		label="Terminal progress bar"
		description="Show a progress bar in the terminal."
		checked={settings.terminalProgressBarEnabled ?? true}
		onCheckedChange={(v) => claudeSettingsStore.update({ terminalProgressBarEnabled: v })}
	/>

	<SettingsToggle
		label="Reduced motion"
		description="Minimize UI animations."
		checked={settings.prefersReducedMotion ?? false}
		onCheckedChange={(v) => claudeSettingsStore.update({ prefersReducedMotion: v })}
	/>

	<SettingsSelect
		label="Notification channel"
		description="Where completion notifications go."
		options={notifChannelOptions}
		value={settings.preferredNotifChannel ?? 'terminal'}
		onValueChange={(v) =>
			claudeSettingsStore.update({
				preferredNotifChannel: v as typeof settings.preferredNotifChannel
			})}
	/>
</SettingsSection>

<SettingsSection title="Files and housekeeping">
	<SettingsToggle
		label="Respect .gitignore"
		description="The file picker skips ignored files."
		checked={settings.respectGitignore ?? true}
		onCheckedChange={(v) => claudeSettingsStore.update({ respectGitignore: v })}
	/>

	<SettingsRow label="Cleanup period" description="Days to keep old conversations.">
		{#snippet control()}
			<Input
				type="number"
				class="h-8 w-24 text-xs"
				aria-label="Cleanup period in days"
				value={String(settings.cleanupPeriodDays ?? 30)}
				oninput={(e) => {
					const val = parseInt(e.currentTarget.value);
					if (!isNaN(val) && val > 0) {
						claudeSettingsStore.update({ cleanupPeriodDays: val });
					}
				}}
			/>
		{/snippet}
	</SettingsRow>

	<SettingsSelect
		label="Updates channel"
		description="Release channel for auto-updates."
		options={updateChannelOptions}
		value={settings.autoUpdatesChannel ?? 'latest'}
		onValueChange={(v) =>
			claudeSettingsStore.update({ autoUpdatesChannel: v as typeof settings.autoUpdatesChannel })}
	/>

	<SettingsToggle
		label="Disable all hooks"
		description="Turn off every hook and custom status line."
		checked={settings.disableAllHooks ?? false}
		onCheckedChange={(v) => claudeSettingsStore.update({ disableAllHooks: v })}
	/>
</SettingsSection>
