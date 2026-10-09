<script lang="ts">
	import NativeTerminalPane from '$features/terminal/NativeTerminalPane.svelte';
	import PaneState from '$features/terminal/PaneState.svelte';
	import { attachable } from '$features/terminal/pane-status';
	import type { TerminalTabState } from '$types/workbench';

	let {
		tab,
		active,
		cwd
	}: {
		tab: TerminalTabState;
		active: boolean;
		cwd: string;
	} = $props();

	// In native mode, only render the first pane (no splits)
	let primaryPane = $derived(tab.panes[0]);
</script>

{#if primaryPane}
	<div class="flex min-h-0 flex-1">
		<div class="relative min-h-0 min-w-0 flex-1">
			{#if attachable(primaryPane)}
				{#key primaryPane.terminalId}
					<NativeTerminalPane
						sessionId={primaryPane.id}
						terminalId={primaryPane.terminalId}
						{active}
					/>
				{/key}
			{:else}
				<PaneState pane={primaryPane} tabId={tab.id} {cwd} />
			{/if}
		</div>
	</div>
{/if}
