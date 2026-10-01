<script lang="ts">
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import CheckIcon from '@lucide/svelte/icons/check';
	import XIcon from '@lucide/svelte/icons/x';
	import { Badge } from '@workbench/ui/badge';
	import { getTrelloStore, getWorkbenchSettingsStore } from '$stores/context';
	import SettingsBoardConfig from './SettingsBoardConfig.svelte';
	import SettingsNote from './SettingsNote.svelte';
	import SettingsRow from './SettingsRow.svelte';
	import SettingsSection from './SettingsSection.svelte';
	import SettingsToggle from './SettingsToggle.svelte';
	import SettingsTrelloAuth from './SettingsTrelloAuth.svelte';

	let { projectPath }: { projectPath: string | null } = $props();

	const store = getWorkbenchSettingsStore();
	const trelloStore = getTrelloStore();

	let ghAuthenticated: boolean | null = $state(null);

	onMount(async () => {
		ghAuthenticated = await invoke<boolean>('github_is_available');
	});
</script>

<SettingsSection title="GitHub">
	<SettingsRow
		label="GitHub CLI"
		description="PRs, CI runs and checks in the sidebar, and cloning repositories. Uses your gh login."
	>
		{#snippet control()}
			{#if ghAuthenticated === null}
				<Badge variant="outline" class="text-[11px] text-wb-ink-mute">Checking…</Badge>
			{:else if ghAuthenticated}
				<Badge variant="outline" class="gap-1 border-wb-ok/40 text-[11px] text-wb-ok">
					<CheckIcon class="size-3" />
					Connected
				</Badge>
			{:else}
				<Badge variant="outline" class="gap-1 border-wb-err/40 text-[11px] text-wb-err">
					<XIcon class="size-3" />
					Not signed in
				</Badge>
			{/if}
		{/snippet}
		{#if ghAuthenticated === false}
			<SettingsNote>
				Run <code>gh auth login</code> in a terminal to connect your account.
			</SettingsNote>
		{/if}
	</SettingsRow>
</SettingsSection>

<SettingsSection title="Trello">
	<SettingsToggle
		label="Trello"
		description="Link boards to projects."
		checked={store.trelloEnabled}
		onCheckedChange={(v) => store.set('trelloEnabled', v)}
	/>
	{#if store.trelloEnabled}
		<div class="px-3.5 py-3">
			<SettingsTrelloAuth />
		</div>
		{#if trelloStore.authenticated}
			<div class="px-3.5 py-3">
				{#if projectPath}
					<SettingsBoardConfig {projectPath} />
				{:else}
					<p class="text-xs text-wb-ink-mute">Open a project to choose its board.</p>
				{/if}
			</div>
		{/if}
	{/if}
</SettingsSection>
