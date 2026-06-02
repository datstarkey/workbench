<script lang="ts">
	import SettingsTrelloAuth from './SettingsTrelloAuth.svelte';
	import SettingsBoardConfig from './SettingsBoardConfig.svelte';
	import { Separator } from '$lib/components/ui/separator';
	import { getTrelloStore } from '$stores/context';

	let { projectPath }: { projectPath: string | null } = $props();

	const trelloStore = getTrelloStore();
</script>

<div class="space-y-4">
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
</div>
