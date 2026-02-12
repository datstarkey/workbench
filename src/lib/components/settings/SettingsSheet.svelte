<script lang="ts">
	import * as Sheet from '$lib/components/ui/sheet';
	import * as Tabs from '$lib/components/ui/tabs';
	import { Button } from '$lib/components/ui/button';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { Badge } from '$lib/components/ui/badge';
	import SaveIcon from '@lucide/svelte/icons/save';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import { getClaudeSettingsStore } from '$stores/context';
	import type { ScopeGroup } from '$types/claude-settings';

	const claudeSettingsStore = getClaudeSettingsStore();
	import SettingsGeneral from './SettingsGeneral.svelte';
	import SettingsPlugins from './SettingsPlugins.svelte';
	import SettingsMcp from './SettingsMcp.svelte';
	import SettingsPermissions from './SettingsPermissions.svelte';
	import SettingsHooks from './SettingsHooks.svelte';
	import SettingsSandbox from './SettingsSandbox.svelte';
	import SettingsSkills from './SettingsSkills.svelte';

	let {
		open = $bindable(false),
		projectPath
	}: {
		open: boolean;
		projectPath: string | null;
	} = $props();

	let activeSection = $state('general');

	const sections = [
		{ id: 'general', label: 'General' },
		{ id: 'permissions', label: 'Permissions' },
		{ id: 'plugins', label: 'Plugins' },
		{ id: 'mcp', label: 'MCP Servers' },
		{ id: 'hooks', label: 'Hooks' },
		{ id: 'sandbox', label: 'Sandbox' },
		{ id: 'skills', label: 'Skills' }
	];

	// Reload when sheet opens or projectPath changes while open
	$effect(() => {
		if (open) {
			// Track projectPath so we reload if active workspace changes while open
			void projectPath;
			claudeSettingsStore.load(projectPath);
		}
	});

	async function handleSave() {
		await claudeSettingsStore.save();
	}
</script>

<Sheet.Root bind:open>
	<Sheet.Content
		side="right"
		class="flex w-[560px] max-w-[90vw] flex-col gap-0 p-0 sm:max-w-[560px]"
	>
		<Sheet.Header class="shrink-0 border-b border-border/60 px-4 py-3">
			<div class="flex items-center justify-between pr-6">
				<Sheet.Title class="text-base">Claude Code Settings</Sheet.Title>
				<div class="flex items-center gap-2">
					{#if claudeSettingsStore.dirty}
						<Badge variant="secondary" class="text-[10px]">unsaved</Badge>
					{/if}
					<Button
						variant="outline"
						size="sm"
						class="h-7 gap-1.5"
						disabled={!claudeSettingsStore.dirty || claudeSettingsStore.saving}
						onclick={handleSave}
					>
						{#if claudeSettingsStore.saving}
							<LoaderIcon class="size-3 animate-spin" />
						{:else}
							<SaveIcon class="size-3" />
						{/if}
						Save
					</Button>
				</div>
			</div>
			<Sheet.Description class="sr-only">
				Manage Claude Code configuration settings
			</Sheet.Description>

			<!-- Scope selector -->
			<div class="mt-3 flex items-center gap-2">
				<Tabs.Root
					value={claudeSettingsStore.activeScopeGroup}
					onValueChange={(v) => claudeSettingsStore.setScopeGroup(v as ScopeGroup)}
				>
					<Tabs.List class="h-8">
						<Tabs.Trigger value="user" class="text-xs">User</Tabs.Trigger>
						<Tabs.Trigger value="project" class="text-xs" disabled={!projectPath}>
							Project
						</Tabs.Trigger>
					</Tabs.List>
				</Tabs.Root>

				<label class="flex items-center gap-1.5 text-xs text-muted-foreground">
					<input
						type="checkbox"
						class="rounded"
						checked={claudeSettingsStore.localOnly}
						onchange={(e) =>
							claudeSettingsStore.setLocalOnly((e.target as HTMLInputElement).checked)}
					/>
					Local only
				</label>
			</div>
		</Sheet.Header>

		<div class="flex min-h-0 flex-1">
			<!-- Section nav -->
			<div
				class="flex w-36 shrink-0 flex-col gap-0.5 border-r border-border/60 p-2"
				role="tablist"
				aria-label="Settings sections"
			>
				{#each sections as section (section.id)}
					<button
						type="button"
						role="tab"
						aria-selected={activeSection === section.id}
						class="rounded-md px-2 py-1.5 text-left text-xs transition-colors {activeSection ===
						section.id
							? 'bg-accent font-medium text-accent-foreground'
							: 'text-muted-foreground hover:bg-accent/50 hover:text-foreground'}"
						onclick={() => (activeSection = section.id)}
					>
						{section.label}
					</button>
				{/each}
			</div>

			<!-- Section content -->
			<ScrollArea class="flex-1">
				<div class="p-4">
					{#if !claudeSettingsStore.loaded}
						<div class="flex items-center justify-center py-12">
							<LoaderIcon class="size-5 animate-spin text-muted-foreground" />
						</div>
					{:else if activeSection === 'general'}
						<SettingsGeneral />
					{:else if activeSection === 'permissions'}
						<SettingsPermissions />
					{:else if activeSection === 'plugins'}
						<SettingsPlugins />
					{:else if activeSection === 'mcp'}
						<SettingsMcp />
					{:else if activeSection === 'hooks'}
						<SettingsHooks />
					{:else if activeSection === 'sandbox'}
						<SettingsSandbox />
					{:else if activeSection === 'skills'}
						<SettingsSkills />
					{/if}
				</div>
			</ScrollArea>
		</div>
	</Sheet.Content>
</Sheet.Root>
