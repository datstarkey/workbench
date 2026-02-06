<script lang="ts">
	import { Switch } from '$lib/components/ui/switch';
	import { Badge } from '$lib/components/ui/badge';
	import { claudeSettingsStore } from '$stores/claude-settings.svelte';

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

<div class="space-y-4">
	<p class="text-xs text-muted-foreground">
		Manage plugins installed in <code class="rounded bg-muted px-1">~/.claude/plugins/</code>.
	</p>

	{#if claudeSettingsStore.plugins.length === 0}
		<div class="rounded-md border border-dashed border-border/60 py-8 text-center">
			<p class="text-sm text-muted-foreground">No plugins found.</p>
			<p class="mt-1 text-xs text-muted-foreground/60">Install plugins via Claude Code CLI.</p>
		</div>
	{:else}
		<div class="space-y-2">
			{#each claudeSettingsStore.plugins as plugin (plugin.dirName)}
				<div class="flex items-center justify-between rounded-md border border-border/60 px-3 py-2">
					<div class="min-w-0 flex-1">
						<div class="flex items-center gap-2">
							<span class="text-sm font-medium">{plugin.name}</span>
							{#if plugin.version}
								<Badge variant="secondary" class="text-[10px]">v{plugin.version}</Badge>
							{/if}
						</div>
						{#if plugin.description}
							<p class="mt-0.5 line-clamp-1 text-xs text-muted-foreground">
								{plugin.description}
							</p>
						{/if}
					</div>
					<Switch
						checked={isEnabled(plugin.dirName)}
						onCheckedChange={(v) => togglePlugin(plugin.dirName, v)}
					/>
				</div>
			{/each}
		</div>
	{/if}
</div>
