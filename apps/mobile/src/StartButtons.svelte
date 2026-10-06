<script lang="ts">
	import { AgentIcon } from '@workbench/ui/agent-icon';
	import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
	import type { MobileClient } from './client.svelte.ts';

	/** Start Claude, Codex or a terminal in a project folder; `wide` fills the row with labels. */
	let {
		client,
		projectPath,
		worktreePath,
		name,
		wide = false
	}: {
		client: MobileClient;
		projectPath: string;
		worktreePath: string | undefined;
		name: string;
		wide?: boolean;
	} = $props();
</script>

<div class={['flex shrink-0 overflow-hidden rounded-lg', wide && 'w-full']}>
	<button
		type="button"
		class={[
			'flex h-9 items-center justify-center gap-1.5 bg-wb-accent px-3 text-[13px] font-semibold text-wb-accent-ink active:brightness-90',
			wide && 'flex-1'
		]}
		aria-label="Start Claude in {name}"
		onclick={() => client.startClaude(projectPath, worktreePath, name)}
	>
		<AgentIcon agent="claude" class="size-4 text-current" />Claude
	</button>
	<button
		type="button"
		class={[
			'flex h-9 items-center justify-center gap-1.5 border border-l-0 border-wb-hair bg-wb-panel2 px-2.5 text-[13px] font-semibold text-wb-codex active:bg-wb-panel',
			wide && 'flex-1'
		]}
		aria-label="Start a Codex chat in {name}"
		onclick={() => client.startCodex(projectPath, worktreePath, name)}
	>
		<AgentIcon agent="codex" class="size-4" />Codex
	</button>
	<button
		type="button"
		class={[
			'flex h-9 items-center justify-center gap-1.5 border border-l-0 border-wb-hair bg-wb-panel2 text-[13px] font-semibold text-wb-ink-mute active:bg-wb-panel',
			wide ? 'flex-1' : 'w-9'
		]}
		aria-label="Open a terminal in {name}"
		onclick={() => client.createTerminal(projectPath, worktreePath, name)}
	>
		<SquareTerminalIcon class="size-4" />
		{#if wide}<span aria-hidden="true">Terminal</span>{/if}
	</button>
</div>
