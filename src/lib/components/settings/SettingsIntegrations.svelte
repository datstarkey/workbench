<script lang="ts">
	import SettingsToggle from './SettingsToggle.svelte';
	import SettingsTrelloAuth from './SettingsTrelloAuth.svelte';
	import SettingsBoardConfig from './SettingsBoardConfig.svelte';
	import { Separator } from '$lib/components/ui/separator';
	import { getWorkbenchSettingsStore, getTrelloStore } from '$stores/context';

	let { projectPath }: { projectPath: string | null } = $props();

	const workbenchSettings = getWorkbenchSettingsStore();
	const trelloStore = getTrelloStore();
</script>

<div class="space-y-6">
	<!-- Trello section -->
	<div class="space-y-4">
		<SettingsToggle
			label="Trello"
			description="Connect your Trello boards to projects"
			checked={workbenchSettings.trelloEnabled}
			onCheckedChange={(v) => workbenchSettings.setTrelloEnabled(v)}
		/>

		{#if workbenchSettings.trelloEnabled}
			<Separator />
			<SettingsTrelloAuth />

			{#if trelloStore.authenticated}
				{#if projectPath}
					<Separator />
					<SettingsBoardConfig {projectPath} />
				{:else}
					<Separator />
					<p class="text-xs text-muted-foreground">Open a project to configure board integration</p>
				{/if}
			{/if}
		{/if}
	</div>
</div>
