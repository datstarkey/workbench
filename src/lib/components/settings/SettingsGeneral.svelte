<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import { claudeSettingsStore } from '$stores/claude-settings.svelte';
	import SettingsSelect from './SettingsSelect.svelte';
	import SettingsToggle from './SettingsToggle.svelte';

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
		{ value: 'terminal_bell', label: 'Terminal Bell' }
	];

	let settings = $derived(claudeSettingsStore.currentSettings);
</script>

<div class="space-y-6">
	<SettingsSelect
		label="Effort Level"
		description="Controls how much effort Claude puts into responses."
		options={effortOptions}
		value={settings.effortLevel ?? 'high'}
		onValueChange={(v) => claudeSettingsStore.update({ effortLevel: v })}
	/>

	<SettingsToggle
		label="Always Enable Thinking"
		description="Use extended thinking by default."
		checked={settings.alwaysThinkingEnabled ?? false}
		onCheckedChange={(v) => claudeSettingsStore.update({ alwaysThinkingEnabled: v })}
	/>

	<SettingsSelect
		label="Updates Channel"
		description="Release channel for auto-updates."
		options={updateChannelOptions}
		value={settings.autoUpdatesChannel ?? 'latest'}
		onValueChange={(v) => claudeSettingsStore.update({ autoUpdatesChannel: v })}
	/>

	<SettingsSelect
		label="Notification Channel"
		description="Where to send completion notifications."
		options={notifChannelOptions}
		value={settings.preferredNotifChannel ?? 'terminal'}
		onValueChange={(v) => claudeSettingsStore.update({ preferredNotifChannel: v })}
	/>

	<div>
		<h3 class="text-sm font-medium">Cleanup Period</h3>
		<p class="mt-1 text-xs text-muted-foreground">Days to keep old conversations before cleanup.</p>
		<div class="mt-2">
			<Input
				type="number"
				class="w-24"
				value={String(settings.cleanupPeriodDays ?? 30)}
				oninput={(e) => {
					const val = parseInt((e.target as HTMLInputElement).value);
					if (!isNaN(val) && val > 0) {
						claudeSettingsStore.update({ cleanupPeriodDays: val });
					}
				}}
			/>
		</div>
	</div>

	<div>
		<h3 class="text-sm font-medium">Language</h3>
		<p class="mt-1 text-xs text-muted-foreground">Preferred response language.</p>
		<div class="mt-2">
			<Input
				class="w-48"
				placeholder="e.g. english, japanese"
				value={settings.language ?? ''}
				oninput={(e) => {
					const val = (e.target as HTMLInputElement).value;
					claudeSettingsStore.update({ language: val || undefined });
				}}
			/>
		</div>
	</div>

	<SettingsToggle
		label="Show Turn Duration"
		description="Display how long each turn takes."
		checked={settings.showTurnDuration ?? true}
		onCheckedChange={(v) => claudeSettingsStore.update({ showTurnDuration: v })}
	/>

	<SettingsToggle
		label="Spinner Tips"
		description="Show tips while Claude is working."
		checked={settings.spinnerTipsEnabled ?? true}
		onCheckedChange={(v) => claudeSettingsStore.update({ spinnerTipsEnabled: v })}
	/>

	<SettingsToggle
		label="Terminal Progress Bar"
		description="Show progress bar in terminal."
		checked={settings.terminalProgressBarEnabled ?? true}
		onCheckedChange={(v) => claudeSettingsStore.update({ terminalProgressBarEnabled: v })}
	/>

	<SettingsToggle
		label="Reduced Motion"
		description="Minimize UI animations for accessibility."
		checked={settings.prefersReducedMotion ?? false}
		onCheckedChange={(v) => claudeSettingsStore.update({ prefersReducedMotion: v })}
	/>

	<SettingsToggle
		label="Respect .gitignore"
		description="File picker respects .gitignore when suggesting files."
		checked={settings.respectGitignore ?? true}
		onCheckedChange={(v) => claudeSettingsStore.update({ respectGitignore: v })}
	/>

	<SettingsToggle
		label="Disable All Hooks"
		description="Turn off all hooks and custom status lines."
		checked={settings.disableAllHooks ?? false}
		onCheckedChange={(v) => claudeSettingsStore.update({ disableAllHooks: v })}
	/>
</div>
