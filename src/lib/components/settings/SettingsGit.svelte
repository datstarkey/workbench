<script lang="ts">
	import SettingsToggle from './SettingsToggle.svelte';
	import { getWorkbenchSettingsStore } from '$stores/context';
	const store = getWorkbenchSettingsStore();
</script>

<div class="space-y-6">
	<SettingsToggle
		label="Git sidebar"
		description="Show the Git tab in the right sidebar for staging, commits, branches, and stashes."
		checked={store.gitSidebarEnabled}
		onCheckedChange={(v) => store.setGitSidebarEnabled(v)}
	/>
</div>
