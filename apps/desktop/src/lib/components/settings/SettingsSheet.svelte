<script lang="ts">
	import { ScrollArea } from '@workbench/ui/scroll-area';
	import * as Dialog from '@workbench/ui/dialog';
	import { getClaudeSettingsStore, getWorkbenchSettingsStore } from '$stores/context';
	import type { ScopeGroup } from '$types/claude-settings';
	import { baseName } from '$lib/utils/path';
	import { watch } from 'runed';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import SaveIcon from '@lucide/svelte/icons/save';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import LayoutListIcon from '@lucide/svelte/icons/layout-list';
	import ZapIcon from '@lucide/svelte/icons/zap';
	import XIcon from '@lucide/svelte/icons/x';

	import SettingsAgentActions from './SettingsAgentActions.svelte';
	import SettingsGeneral from './SettingsGeneral.svelte';
	import SettingsHooks from './SettingsHooks.svelte';
	import SettingsIntegrations from './SettingsIntegrations.svelte';
	import SettingsMcp from './SettingsMcp.svelte';
	import SettingsPermissions from './SettingsPermissions.svelte';
	import SettingsPlugins from './SettingsPlugins.svelte';
	import SettingsSandbox from './SettingsSandbox.svelte';
	import SettingsSkills from './SettingsSkills.svelte';
	import SettingsWorkbench from './SettingsWorkbench.svelte';

	const claudeSettingsStore = getClaudeSettingsStore();
	const workbenchSettingsStore = getWorkbenchSettingsStore();

	let {
		open = $bindable(false),
		projectPath
	}: {
		open: boolean;
		projectPath: string | null;
	} = $props();

	type SettingsTab = 'general' | 'claude' | 'trello' | 'agent-actions';

	let selectedTab = $state<SettingsTab>('general');
	let activeSection = $state('general');

	// Falls back to 'general' if Trello tab is selected but feature is disabled
	let settingsMode = $derived<SettingsTab>(
		selectedTab === 'trello' && !workbenchSettingsStore.trelloEnabled ? 'general' : selectedTab
	);

	const navItems = $derived([
		{ id: 'general' as const, label: 'General', icon: SettingsIcon },
		{ id: 'claude' as const, label: 'Claude Code', icon: SparklesIcon },
		...(workbenchSettingsStore.trelloEnabled
			? [{ id: 'trello' as const, label: 'Integrations', icon: LayoutListIcon }]
			: []),
		{ id: 'agent-actions' as const, label: 'Agent Actions', icon: ZapIcon }
	]);

	const claudeSections = [
		{ id: 'general', label: 'General' },
		{ id: 'permissions', label: 'Permissions' },
		{ id: 'plugins', label: 'Plugins' },
		{ id: 'mcp', label: 'MCP Servers' },
		{ id: 'hooks', label: 'Hooks' },
		{ id: 'sandbox', label: 'Sandbox' },
		{ id: 'skills', label: 'Skills' }
	];

	let activeStore = $derived(
		settingsMode === 'claude' ? claudeSettingsStore : workbenchSettingsStore
	);

	const contextLabel = $derived(projectPath ? baseName(projectPath) : '');
	const scopeLabel = $derived(
		settingsMode === 'claude' ? claudeSettingsStore.activeScopeGroup : 'workbench'
	);

	// Reload when modal opens or projectPath changes while open.
	// watch() tracks open + projectPath; the load call runs untracked.
	watch(
		() => [open, projectPath] as const,
		([isOpen, path]) => {
			if (isOpen) claudeSettingsStore.load(path);
		}
	);

	async function handleSave() {
		await activeStore.save();
	}

	async function handleReset() {
		if (settingsMode === 'claude') {
			await claudeSettingsStore.load(projectPath);
		} else {
			await workbenchSettingsStore.load();
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content
		showCloseButton={false}
		class="flex h-[560px] w-[840px] max-w-[90vw] flex-col gap-0 overflow-hidden rounded-[10px] border-wb-hair bg-wb-panel p-0 text-wb-ink shadow-[0_32px_80px_rgba(0,0,0,0.5)]"
	>
		<Dialog.Title class="sr-only">Settings</Dialog.Title>
		<Dialog.Description class="sr-only">
			Manage Workbench and Claude Code configuration settings
		</Dialog.Description>

		<!-- Header -->
		<div class="flex h-12 flex-shrink-0 items-center gap-2.5 border-b border-wb-hair px-4.5">
			<SettingsIcon size={15} class="text-wb-accent" />
			<span class="text-[13.5px] font-semibold">Settings</span>
			{#if contextLabel}
				<span class="font-mono text-[11.5px] text-wb-ink-soft">{contextLabel}</span>
			{/if}
			<span class="flex-1"></span>
			{#if activeStore.dirty}
				<span class="rounded bg-wb-warn/20 px-1.5 py-0.5 font-mono text-[10px] text-wb-warn">
					unsaved
				</span>
			{/if}
			<Dialog.Close
				class="grid size-6 place-items-center rounded text-wb-ink-mute hover:text-wb-ink"
			>
				<XIcon size={13} />
			</Dialog.Close>
		</div>

		<!-- Body -->
		<div class="flex min-h-0 flex-1">
			<!-- Left nav rail -->
			<div
				class="flex w-[210px] flex-shrink-0 flex-col gap-0.5 border-r border-wb-hair bg-wb-bg p-2"
				role="tablist"
				aria-label="Settings sections"
			>
				{#each navItems as item (item.id)}
					{@const Icon = item.icon}
					{@const active = settingsMode === item.id}
					<button
						type="button"
						role="tab"
						aria-selected={active}
						class="flex items-center gap-2.5 rounded-md px-2.5 py-[7px] text-left text-[12.5px] transition-colors {active
							? 'bg-wb-panel2 text-wb-ink'
							: 'text-wb-ink-mute hover:text-wb-ink'}"
						onclick={() => (selectedTab = item.id)}
					>
						<Icon size={13} class={active ? 'text-wb-accent' : 'text-wb-ink-soft'} />
						<span class="flex-1 truncate">{item.label}</span>
					</button>
					{#if item.id === 'claude' && active}
						<div
							class="mb-1 ml-3 flex flex-col gap-0.5 border-l border-wb-hair pl-2"
							role="tablist"
							aria-label="Claude Code sections"
						>
							{#each claudeSections as section (section.id)}
								<button
									type="button"
									role="tab"
									aria-selected={activeSection === section.id}
									class="rounded px-2 py-1 text-left text-[12px] transition-colors {activeSection ===
									section.id
										? 'text-wb-ink'
										: 'text-wb-ink-soft hover:text-wb-ink-mute'}"
									onclick={() => (activeSection = section.id)}
								>
									{section.label}
								</button>
							{/each}
						</div>
					{/if}
				{/each}
			</div>

			<!-- Form area -->
			<div class="flex min-w-0 flex-1 flex-col">
				{#if settingsMode === 'claude'}
					<!-- Claude scope selector -->
					<div
						class="flex flex-shrink-0 items-center gap-3 border-b border-wb-hair-soft px-5 py-2.5"
					>
						<div class="inline-flex rounded border border-wb-hair bg-wb-bg p-0.5">
							<button
								type="button"
								class="rounded-sm px-2.5 py-1 text-[11.5px] transition-colors {claudeSettingsStore.activeScopeGroup ===
								'user'
									? 'bg-wb-panel2 text-wb-ink'
									: 'text-wb-ink-mute'}"
								onclick={() => claudeSettingsStore.setScopeGroup('user' as ScopeGroup)}
							>
								User
							</button>
							<button
								type="button"
								disabled={!projectPath}
								class="rounded-sm px-2.5 py-1 text-[11.5px] transition-colors disabled:opacity-40 {claudeSettingsStore.activeScopeGroup ===
								'project'
									? 'bg-wb-panel2 text-wb-ink'
									: 'text-wb-ink-mute'}"
								onclick={() => claudeSettingsStore.setScopeGroup('project' as ScopeGroup)}
							>
								Project
							</button>
						</div>
						<label class="flex items-center gap-1.5 text-[11.5px] text-wb-ink-mute">
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
				{/if}

				<ScrollArea class="min-h-0 flex-1">
					<div class="p-6">
						{#if settingsMode === 'general'}
							{#if !workbenchSettingsStore.loaded}
								<div class="flex items-center justify-center py-12">
									<LoaderIcon class="size-5 animate-spin text-wb-ink-soft" />
								</div>
							{:else}
								<SettingsWorkbench />
							{/if}
						{:else if settingsMode === 'claude'}
							{#if !claudeSettingsStore.loaded}
								<div class="flex items-center justify-center py-12">
									<LoaderIcon class="size-5 animate-spin text-wb-ink-soft" />
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
						{:else if settingsMode === 'trello'}
							<SettingsIntegrations {projectPath} />
						{:else if settingsMode === 'agent-actions'}
							{#if !workbenchSettingsStore.loaded}
								<div class="flex items-center justify-center py-12">
									<LoaderIcon class="size-5 animate-spin text-wb-ink-soft" />
								</div>
							{:else}
								<SettingsAgentActions />
							{/if}
						{/if}
					</div>
				</ScrollArea>
			</div>
		</div>

		<!-- Footer -->
		<div class="flex h-12 flex-shrink-0 items-center gap-2 border-t border-wb-hair px-4.5">
			<span class="font-mono text-[11px] text-wb-ink-soft">
				scope <span class="text-wb-accent">{scopeLabel}</span>
			</span>
			<span class="flex-1"></span>
			<button
				type="button"
				class="flex items-center gap-1.5 rounded border border-wb-hair px-3.5 py-1.5 text-[12px] text-wb-ink transition-colors hover:bg-wb-panel2 disabled:opacity-40"
				disabled={!activeStore.dirty || activeStore.saving}
				onclick={handleReset}
			>
				<RotateCcwIcon size={12} />
				Reset
			</button>
			<button
				type="button"
				class="flex items-center gap-1.5 rounded bg-primary px-3.5 py-1.5 text-[12px] font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-40"
				disabled={!activeStore.dirty || activeStore.saving}
				onclick={handleSave}
			>
				{#if activeStore.saving}
					<LoaderIcon class="size-3 animate-spin" />
				{:else}
					<SaveIcon class="size-3" />
				{/if}
				Save changes
			</button>
		</div>
	</Dialog.Content>
</Dialog.Root>
