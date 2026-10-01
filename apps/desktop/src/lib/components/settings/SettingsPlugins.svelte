<script lang="ts">
	import { getClaudeSettingsStore } from '$stores/context';
	import SettingsEmptyState from './SettingsEmptyState.svelte';
	import SettingsSection from './SettingsSection.svelte';
	import SettingsToggle from './SettingsToggle.svelte';

	const claudeSettingsStore = getClaudeSettingsStore();

	let settings = $derived(claudeSettingsStore.currentSettings);
	let enabledPlugins = $derived((settings.enabledPlugins ?? []) as string[]);
	let disabledPlugins = $derived((settings.disabledPlugins ?? []) as string[]);

	function isEnabled(dirName: string): boolean {
		if (disabledPlugins.includes(dirName)) return false;
		if (enabledPlugins.includes(dirName)) return true;
		return true; // enabled by default
	}

	function togglePlugin(dirName: string, enabled: boolean) {
		if (enabled) {
			claudeSettingsStore.removeFromList('disabledPlugins', dirName);
			claudeSettingsStore.addToList('enabledPlugins', dirName);
		} else {
			claudeSettingsStore.removeFromList('enabledPlugins', dirName);
			claudeSettingsStore.addToList('disabledPlugins', dirName);
		}
	}
</script>

{#if claudeSettingsStore.plugins.length === 0}
	<SettingsEmptyState
		title="No plugins found."
		subtitle="Install plugins with the Claude Code CLI."
	/>
{:else}
	<SettingsSection>
		{#each claudeSettingsStore.plugins as plugin (plugin.dirName)}
			<SettingsToggle
				label={plugin.version ? `${plugin.name} v${plugin.version}` : plugin.name}
				description={plugin.description}
				checked={isEnabled(plugin.dirName)}
				onCheckedChange={(v) => togglePlugin(plugin.dirName, v)}
			/>
		{/each}
	</SettingsSection>
{/if}
