<script lang="ts">
	import { ScrollArea } from '@workbench/ui/scroll-area';
	import { getClaudeSettingsStore, getWorkbenchSettingsStore } from '$stores/context';
	import { baseName } from '$lib/utils/path';
	import { emit } from '@tauri-apps/api/event';
	import InfoIcon from '@lucide/svelte/icons/info';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import SaveIcon from '@lucide/svelte/icons/save';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';

	import SettingsAgentActions from './SettingsAgentActions.svelte';
	import SettingsBashSandbox from './SettingsBashSandbox.svelte';
	import SettingsBehaviour from './SettingsBehaviour.svelte';
	import SettingsClaudeSessions from './SettingsClaudeSessions.svelte';
	import SettingsCodexSessions from './SettingsCodexSessions.svelte';
	import SettingsGeneral from './SettingsGeneral.svelte';
	import SettingsHooks from './SettingsHooks.svelte';
	import SettingsIntegrations from './SettingsIntegrations.svelte';
	import SettingsMcp from './SettingsMcp.svelte';
	import SettingsNav from './SettingsNav.svelte';
	import SettingsPermissions from './SettingsPermissions.svelte';
	import SettingsPlugins from './SettingsPlugins.svelte';
	import SettingsServerMode from './SettingsServerMode.svelte';
	import SettingsSkills from './SettingsSkills.svelte';
	import SettingsTerminal from './SettingsTerminal.svelte';
	import SettingsWorktrees from './SettingsWorktrees.svelte';
	import {
		WORKBENCH_SETTINGS_FILE,
		claudeSettingsFile,
		settingsPage,
		type SettingsPageId
	} from './settings-pages';

	const claudeSettingsStore = getClaudeSettingsStore();
	const workbenchSettingsStore = getWorkbenchSettingsStore();

	let { projectPath }: { projectPath: string | null } = $props();

	let pageId = $state<SettingsPageId>('general');

	const page = $derived(settingsPage(pageId));
	const isClaudePage = $derived(page.store === 'claude');
	const activeStore = $derived(isClaudePage ? claudeSettingsStore : workbenchSettingsStore);
	const projectName = $derived(projectPath ? baseName(projectPath) : '');
	const savesTo = $derived(
		isClaudePage
			? claudeSettingsFile(claudeSettingsStore.activeScope, projectName)
			: WORKBENCH_SETTINGS_FILE
	);

	async function handleSave() {
		await activeStore.save();
		// Notify the main window so live-bound settings (accent, sidebar toggles,
		// integrations) refresh without a restart.
		await emit('settings:changed');
	}

	async function handleReset() {
		if (isClaudePage) {
			await claudeSettingsStore.load(projectPath);
		} else {
			await workbenchSettingsStore.load();
		}
	}
</script>

<div class="flex h-full min-h-0 bg-wb-panel text-wb-ink">
	<SettingsNav active={pageId} onSelect={(id) => (pageId = id)} />

	<div class="flex min-w-0 flex-1 flex-col">
		<header class="flex shrink-0 items-start gap-4 border-b border-wb-hair-soft px-7 pt-5.5 pb-4">
			<div class="min-w-0 flex-1">
				<h1 class="text-[17px] font-semibold tracking-tight">{page.title ?? page.label}</h1>
				<p class="mt-1 text-[12.5px] leading-relaxed text-wb-ink-mute">{page.description}</p>
			</div>
			{#if isClaudePage}
				<div class="flex shrink-0 flex-col items-end gap-2">
					<div
						class="inline-flex rounded-md border border-wb-hair bg-wb-bg p-0.5"
						role="group"
						aria-label="Settings scope"
					>
						<button
							type="button"
							aria-pressed={claudeSettingsStore.activeScopeGroup === 'user'}
							class={[
								'rounded px-2.5 py-1 text-xs transition-colors',
								claudeSettingsStore.activeScopeGroup === 'user'
									? 'bg-wb-panel2 text-wb-ink'
									: 'text-wb-ink-mute hover:text-wb-ink'
							]}
							onclick={() => claudeSettingsStore.setScopeGroup('user')}
						>
							User
						</button>
						<button
							type="button"
							disabled={!projectPath}
							aria-pressed={claudeSettingsStore.activeScopeGroup === 'project'}
							class={[
								'rounded px-2.5 py-1 text-xs transition-colors disabled:opacity-40',
								claudeSettingsStore.activeScopeGroup === 'project'
									? 'bg-wb-panel2 text-wb-ink'
									: 'text-wb-ink-mute hover:text-wb-ink'
							]}
							onclick={() => claudeSettingsStore.setScopeGroup('project')}
						>
							{projectName ? `Project · ${projectName}` : 'Project'}
						</button>
					</div>
					<label class="flex items-center gap-1.5 text-xs text-wb-ink-mute">
						<input
							type="checkbox"
							class="rounded"
							checked={claudeSettingsStore.localOnly}
							onchange={(e) => claudeSettingsStore.setLocalOnly(e.currentTarget.checked)}
						/>
						Local only
					</label>
				</div>
			{/if}
		</header>

		<ScrollArea class="min-h-0 flex-1">
			<div class="flex flex-col gap-6 px-7 pt-5 pb-7">
				{#if !activeStore.loaded}
					<div class="flex items-center justify-center py-12">
						<LoaderIcon class="size-5 animate-spin text-wb-ink-soft" />
					</div>
				{:else if pageId === 'general'}
					<SettingsGeneral />
				{:else if pageId === 'worktrees'}
					<SettingsWorktrees />
				{:else if pageId === 'terminal'}
					<SettingsTerminal />
				{:else if pageId === 'agent-actions'}
					<SettingsAgentActions />
				{:else if pageId === 'integrations'}
					<SettingsIntegrations {projectPath} />
				{:else if pageId === 'remote-access'}
					<SettingsServerMode />
				{:else if pageId === 'claude-sessions'}
					<SettingsClaudeSessions />
				{:else if pageId === 'behaviour'}
					<SettingsBehaviour />
				{:else if pageId === 'permissions'}
					<SettingsPermissions />
				{:else if pageId === 'bash-sandbox'}
					<SettingsBashSandbox />
				{:else if pageId === 'mcp'}
					<SettingsMcp />
				{:else if pageId === 'plugins'}
					<SettingsPlugins />
				{:else if pageId === 'hooks'}
					<SettingsHooks />
				{:else if pageId === 'skills'}
					<SettingsSkills />
				{:else if pageId === 'codex-sessions'}
					<SettingsCodexSessions />
				{/if}
			</div>
		</ScrollArea>

		<footer class="flex h-12 shrink-0 items-center gap-2 border-t border-wb-hair pr-4 pl-7">
			{#if page.store === 'immediate'}
				<span class="flex flex-1 items-center gap-2 text-[11.5px] text-wb-ink-mute">
					<InfoIcon class="size-3.5" />
					Changes on this page apply immediately.
				</span>
			{:else}
				<span class="flex min-w-0 flex-1 items-center gap-2 text-[11.5px] text-wb-ink-mute">
					{#if activeStore.dirty}
						<span class="size-1.5 shrink-0 rounded-full bg-wb-warn"></span>
						<span class="text-wb-warn">Unsaved changes</span>
					{/if}
					<span class="truncate font-mono">{savesTo}</span>
				</span>
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
			{/if}
		</footer>
	</div>
</div>
