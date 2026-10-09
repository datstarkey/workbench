<script lang="ts">
	import { AgentIcon } from '@workbench/ui/agent-icon';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import type { MobileClient } from './client.svelte.ts';
	import { useBack } from './back-navigation';
	import type { PaneEntry } from './panes.ts';
	import { paneTitle } from './panes.ts';
	import type { PaneKind } from './workspace-stream.ts';

	/**
	 * A pane whose chat or terminal isn't up: starting, asking for trust, or
	 * exited; or (`starting`) a new session the host hasn't shown yet.
	 */
	let {
		client,
		entry,
		starting
	}: { client: MobileClient; entry?: PaneEntry; starting?: PaneKind } = $props();

	const pane = $derived(entry?.pane);
	const kind = $derived(pane?.kind ?? starting ?? 'shell');
	const cwd = $derived(entry ? (entry.workspace.worktreePath ?? entry.workspace.projectPath) : '');
	const title = $derived(
		entry ? paneTitle(entry) : kind === 'shell' ? 'New terminal' : `New ${kind} session`
	);
	const message = $derived(
		pane?.status === 'needsTrust'
			? `Claude Code asks whether you trust ${cwd}.`
			: pane?.status === 'exited'
				? 'This session has ended. Restart it, or End it to close its tab.'
				: kind === 'claude' && pane
					? 'Starting Claude. A login or trust prompt shows in its terminal.'
					: 'Starting…'
	);
	useBack(() => client.closeScreen());

	const button =
		'h-10 rounded-lg border border-wb-hair bg-wb-panel2 px-4 text-[13px] font-medium active:bg-wb-panel';
</script>

<div class="flex h-full flex-col bg-wb-bg text-wb-ink">
	<header
		class="flex shrink-0 items-center gap-2 border-b border-wb-hair bg-wb-rail pr-2 pl-1"
		style="padding-top: env(safe-area-inset-top); min-height: calc(3rem + env(safe-area-inset-top));"
	>
		<button
			type="button"
			class="grid size-9 shrink-0 place-items-center rounded-lg text-wb-ink-mute active:bg-wb-panel2"
			aria-label="Back"
			onclick={client.closeScreen}
		>
			<ChevronLeftIcon class="size-5" />
		</button>
		{#if kind !== 'shell'}<AgentIcon agent={kind} class="size-5" />{/if}
		<span class="truncate text-[14px] font-semibold">{title}</span>
	</header>
	<main class="flex flex-1 flex-col items-center justify-center gap-4 p-6 text-center">
		<p class="text-[13.5px] text-wb-ink-mute" role="status">{message}</p>
		{#if pane?.error}<p class="text-xs text-wb-err" role="alert">{pane.error}</p>{/if}
		{#if client.notice}<p class="text-xs text-wb-err" role="alert">{client.notice}</p>{/if}
		{#if entry && pane}
			<div class="flex flex-wrap justify-center gap-2">
				{#if pane.status === 'needsTrust'}
					<button
						type="button"
						class="h-10 rounded-lg bg-wb-accent px-4 text-[13px] font-semibold text-wb-accent-ink active:brightness-90"
						onclick={() => client.trustFolder(pane.id)}
					>
						Trust folder
					</button>
				{/if}
				{#if pane.status === 'exited' && client.canRestart(pane)}
					<button type="button" class={button} onclick={() => client.restart(entry.tab.id)}>
						Restart
					</button>
				{/if}
				{#if pane.kind === 'claude' && pane.terminalId}
					<button type="button" class={button} onclick={() => client.setView(pane.id, 'terminal')}>
						Show terminal
					</button>
				{/if}
				<button type="button" class="{button} text-wb-err" onclick={() => client.endPane(pane.id)}>
					End
				</button>
			</div>
		{/if}
	</main>
</div>
