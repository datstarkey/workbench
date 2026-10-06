<script lang="ts">
	import NativeTerminalPane from '$features/terminal/NativeTerminalPane.svelte';
	import type { ProjectConfig, TerminalPaneState } from '$types/workbench';
	import { claudeSessionLaunch } from '$lib/utils/claude';

	let {
		panes,
		active,
		project,
		cwd
	}: {
		panes: TerminalPaneState[];
		active: boolean;
		project: ProjectConfig;
		cwd?: string;
	} = $props();

	// In native mode, only render the first pane (no splits)
	let primaryPane = $derived(panes[0]);
</script>

{#if primaryPane}
	<div class="flex min-h-0 flex-1">
		<div class="relative min-h-0 min-w-0 flex-1">
			<NativeTerminalPane
				sessionId={primaryPane.id}
				{project}
				{active}
				{cwd}
				startupCommand={primaryPane.startupCommand}
				claudeSession={claudeSessionLaunch(primaryPane)}
				claudeAccountId={primaryPane.claudeAccountId}
			/>
		</div>
	</div>
{/if}
