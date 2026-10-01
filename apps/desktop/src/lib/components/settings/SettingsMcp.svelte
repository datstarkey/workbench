<script lang="ts">
	import { getClaudeSettingsStore } from '$stores/context';
	import SettingsEmptyState from './SettingsEmptyState.svelte';
	import SettingsSection from './SettingsSection.svelte';
	import SettingsToggle from './SettingsToggle.svelte';
	import type { McpServerConfig } from '$types/claude-settings';

	const claudeSettingsStore = getClaudeSettingsStore();

	let settings = $derived(claudeSettingsStore.currentSettings);
	let servers = $derived((settings.mcpServers ?? {}) as Record<string, McpServerConfig>);
	let serverNames = $derived(Object.keys(servers));

	function toggleServer(name: string, enabled: boolean) {
		const updated = { ...servers };
		updated[name] = { ...updated[name], disabled: !enabled };
		claudeSettingsStore.updateNested('mcpServers', updated);
	}

	function commandLine(server: McpServerConfig): string | undefined {
		if (!server.command) return undefined;
		return [server.command, ...(server.args ?? [])].join(' ');
	}
</script>

<SettingsSection>
	<SettingsToggle
		label="Enable all project MCP servers"
		description="Turn on MCP servers defined in project settings without asking."
		checked={settings.enableAllProjectMcpServers ?? false}
		onCheckedChange={(v) => claudeSettingsStore.update({ enableAllProjectMcpServers: v })}
	/>
</SettingsSection>

{#if serverNames.length === 0}
	<SettingsEmptyState
		title="No MCP servers configured."
		subtitle="Add servers to this settings file."
	/>
{:else}
	<SettingsSection title="Servers">
		{#each serverNames as name (name)}
			<SettingsToggle
				label={name}
				description={commandLine(servers[name])}
				checked={!servers[name].disabled}
				onCheckedChange={(v) => toggleServer(name, v)}
			/>
		{/each}
	</SettingsSection>
{/if}
