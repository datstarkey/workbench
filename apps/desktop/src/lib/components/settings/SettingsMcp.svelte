<script lang="ts">
	import { Label } from '$lib/components/ui/label';
	import { Switch } from '$lib/components/ui/switch';
	import { Badge } from '$lib/components/ui/badge';
	import { getClaudeSettingsStore } from '$stores/context';
	import SettingsEmptyState from './SettingsEmptyState.svelte';
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
</script>

<div class="space-y-6">
	<div class="flex items-center justify-between">
		<div>
			<Label class="text-sm font-medium">Enable All Project MCP Servers</Label>
			<p class="text-xs text-muted-foreground">
				Auto-enable MCP servers defined in project settings.
			</p>
		</div>
		<Switch
			checked={settings.enableAllProjectMcpServers ?? false}
			onCheckedChange={(v) => claudeSettingsStore.update({ enableAllProjectMcpServers: v })}
		/>
	</div>

	<div>
		<h3 class="text-sm font-medium">MCP Servers</h3>
		{#if serverNames.length === 0}
			<div class="mt-2">
				<SettingsEmptyState
					title="No MCP servers configured."
					subtitle="Add servers to your settings JSON file."
				/>
			</div>
		{:else}
			<div class="mt-2 space-y-2">
				{#each serverNames as name (name)}
					{@const server = servers[name]}
					<div
						class="flex items-center justify-between rounded-md border border-border/60 px-3 py-2"
					>
						<div class="min-w-0 flex-1">
							<div class="flex items-center gap-2">
								<span class="text-sm font-medium">{name}</span>
								{#if server.disabled}
									<Badge variant="secondary" class="text-[10px]">disabled</Badge>
								{/if}
							</div>
							{#if server.command}
								<p class="mt-0.5 line-clamp-1 font-mono text-xs text-muted-foreground">
									{server.command}
									{#if server.args}
										{server.args.join(' ')}{/if}
								</p>
							{/if}
						</div>
						<Switch checked={!server.disabled} onCheckedChange={(v) => toggleServer(name, v)} />
					</div>
				{/each}
			</div>
		{/if}
	</div>
</div>
