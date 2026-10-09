<script lang="ts">
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import { getWorkspaceStore } from '$stores/context';

	const workspaceStore = getWorkspaceStore();
	const persistence = $derived(workspaceStore.persistence);
</script>

<!-- Stays while the server can't save the workspaces: tabs opened now are lost on quit. -->
{#if persistence.status !== 'ok'}
	<div
		class="flex shrink-0 items-start gap-2 border-b border-wb-hair bg-wb-warn/10 px-3 py-1.5 text-xs text-wb-ink"
		role="alert"
	>
		<TriangleAlertIcon class="mt-px size-3.5 shrink-0 text-wb-warn" />
		<span>
			<span class="font-medium">Tabs aren't being saved.</span>
			{persistence.message ??
				(persistence.status === 'locked'
					? 'Another Workbench server is using this config folder.'
					: "The saved workspaces couldn't be read.")}
		</span>
	</div>
{/if}
