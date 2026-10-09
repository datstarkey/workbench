<script lang="ts">
	import { isAISessionType, type TerminalPaneState } from '$types/workbench';
	import { getWorkspaceStore } from '$stores/context';

	let { pane, tabId, cwd }: { pane: TerminalPaneState; tabId: string; cwd: string } = $props();

	const workspaceStore = getWorkspaceStore();
	const ai = $derived(isAISessionType(pane.type));
	const name = $derived(
		pane.type === 'codex' ? 'Codex' : pane.type === 'claude' ? 'Claude' : 'Shell'
	);
</script>

<!-- What a pane shows while the server has no running process for it to attach to. -->
<div
	class="absolute inset-0 z-10 flex items-center justify-center bg-wb-bg text-xs text-wb-ink-mute"
>
	<div class="flex max-w-sm flex-col items-center gap-2 px-4 text-center" role="status">
		{#if pane.status === 'needsTrust'}
			<p class="text-wb-ink">Claude Code asks whether to trust this folder</p>
			<p class="font-mono text-[11px] break-all">{cwd}</p>
			<button
				type="button"
				class="mt-1 rounded bg-wb-accent px-3 py-1 text-xs text-white hover:opacity-90"
				onclick={() => workspaceStore.trustFolder(pane.id)}
			>
				Trust folder
			</button>
		{:else if pane.status === 'exited'}
			<p>{pane.error ?? `${name} exited`}</p>
			{#if ai}
				<button
					type="button"
					class="mt-1 rounded bg-white/10 px-3 py-1 text-xs text-wb-ink hover:bg-white/20"
					onclick={() => workspaceStore.restartAISession('', tabId)}
				>
					Restart
				</button>
			{:else}
				<button
					type="button"
					class="mt-1 rounded bg-white/10 px-3 py-1 text-xs text-wb-ink hover:bg-white/20"
					onclick={() => workspaceStore.removePane('', pane.id)}
				>
					Close
				</button>
			{/if}
		{:else if pane.error}
			<p class="text-wb-err">{pane.error}</p>
		{:else}
			<span class="starting" aria-hidden="true"></span>
			<p>Starting {name}…</p>
		{/if}
	</div>
</div>

<style>
	.starting {
		width: 28px;
		height: 3px;
		border-radius: 999px;
		background: var(--wb-ink-soft);
		animation: pulse 1s ease-in-out infinite alternate;
	}
	@keyframes pulse {
		from {
			opacity: 0.3;
		}
		to {
			opacity: 1;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.starting {
			animation: none;
		}
	}
</style>
