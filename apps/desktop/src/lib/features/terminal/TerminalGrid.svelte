<script lang="ts">
	import XIcon from '@lucide/svelte/icons/x';
	import { SvelteMap } from 'svelte/reactivity';
	import { toast } from 'svelte-sonner';
	import { cn } from '@workbench/ui';
	import SessionChat from '$features/chat/SessionChat.svelte';
	import { paneAgent } from '$features/chat/pane-handoff';
	import TerminalPane from '$features/terminal/TerminalPane.svelte';
	import { claudeSessionLaunch } from '$lib/utils/claude';
	import { getWorkspaceStore } from '$stores/context';
	import {
		isAISessionType,
		type PaneView,
		type ProjectConfig,
		type SplitDirection,
		type TerminalPaneState
	} from '$types/workbench';

	const workspaceStore = getWorkspaceStore();

	let {
		workspaceId,
		panes,
		split,
		active,
		project,
		cwd
	}: {
		workspaceId: string;
		panes: TerminalPaneState[];
		split: SplitDirection;
		active: boolean;
		project: ProjectConfig;
		cwd?: string;
	} = $props();

	/** Panes mid-switch, and where they're going. */
	const switching = new SvelteMap<string, PaneView>();

	async function switchView(paneId: string, view: PaneView) {
		if (switching.has(paneId)) return;
		switching.set(paneId, view);
		try {
			await workspaceStore.setPaneView(paneId, view);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : String(e));
		} finally {
			switching.delete(paneId);
		}
	}
</script>

<div class={`flex min-h-0 flex-1 ${split === 'vertical' ? 'flex-col' : 'flex-row'}`}>
	{#each panes as pane, i (pane.id)}
		{@const agent = isAISessionType(pane.type) ? paneAgent(pane) : null}
		{@const chatSessionId = agent ? pane.claudeSessionId || undefined : undefined}
		{@const canChat = agent === 'codex' || Boolean(chatSessionId)}
		{@const inChat = canChat && pane.view === 'chat'}
		{@const target = switching.get(pane.id)}
		{#if i > 0}
			<div
				class={split === 'vertical' ? 'h-px shrink-0 bg-border/60' : 'w-px shrink-0 bg-border/60'}
			></div>
		{/if}
		<div class="relative min-h-0 min-w-0 flex-1">
			{#if inChat && agent}
				<!-- Only while visible: every grid stays mounted, and a hidden chat would keep streaming. -->
				{#if active}
					{#key chatSessionId}
						<SessionChat
							{agent}
							paneId={pane.id}
							sessionId={chatSessionId}
							{project}
							{cwd}
							claudeAccountId={pane.claudeAccountId}
							onShowTerminal={() => switchView(pane.id, 'terminal')}
							onSessionIdChange={(id) => workspaceStore.updateAISessionByPaneId(pane.id, id, agent)}
						/>
					{/key}
				{/if}
			{/if}
			<!-- A live terminal's chat is the same `claude`: keep its xterm attached underneath. -->
			{#if !(inChat && agent) || pane.liveTerminal}
				<div class={['h-full', inChat && 'hidden']}>
					<!-- A rewind restarts a live chat's terminal: follow it to the new one. -->
					{#key pane.liveTerminal ? workspaceStore.getServerTerminalId(pane.id) : pane.id}
						<TerminalPane
							sessionId={pane.id}
							{project}
							active={active && !inChat}
							{cwd}
							startupCommand={pane.startupCommand}
							claudeSession={claudeSessionLaunch(pane)}
							claudeAccountId={pane.claudeAccountId}
							existingServerTerminalId={workspaceStore.getServerTerminalId(pane.id)}
							onServerTerminalIdChange={(paneId, serverTerminalId) =>
								workspaceStore.setServerTerminalId(paneId, serverTerminalId)}
						/>
					{/key}
				</div>
			{/if}
			{#if target}
				<div
					class="absolute inset-0 z-30 flex items-center justify-center bg-wb-bg/85 backdrop-blur-[2px]"
					style:--agent={agent === 'codex' ? 'var(--wb-codex)' : 'var(--wb-claude)'}
					role="status"
				>
					<div class="flex items-center gap-2.5 text-xs text-wb-ink-mute">
						<span class="handoff" aria-hidden="true"></span>
						{target === 'chat' ? 'Moving this session to chat…' : 'Reopening in the terminal…'}
					</div>
				</div>
			{/if}
			<!-- A Codex pane can chat before it has a thread: chat starts one. -->
			{#if canChat}
				<div
					class="absolute top-1.5 right-10 z-20 flex overflow-hidden rounded-md border border-wb-hair bg-wb-panel/90 text-[11px] backdrop-blur-sm"
					role="group"
					aria-label="Session view"
				>
					{#each ['terminal', 'chat'] as const as view (view)}
						{@const selected = (pane.view ?? 'terminal') === view}
						<button
							type="button"
							class={cn(
								'px-2 py-0.5 capitalize focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none focus-visible:ring-inset',
								!selected && 'text-wb-ink-mute hover:text-wb-ink',
								selected &&
									(agent === 'codex'
										? 'bg-wb-codex/20 text-wb-codex'
										: 'bg-wb-accent-soft text-wb-ink')
							)}
							aria-pressed={selected}
							disabled={Boolean(target)}
							onclick={() => switchView(pane.id, view)}
						>
							{view}
						</button>
					{/each}
				</div>
			{/if}
			{#if panes.length > 1}
				<button
					class="absolute top-2 right-2 z-20 flex size-6 items-center justify-center rounded bg-background/80 text-muted-foreground opacity-0 backdrop-blur-sm transition-opacity hover:text-foreground [div:hover>&]:opacity-100"
					type="button"
					aria-label="Close pane"
					onclick={() => workspaceStore.removePane(workspaceId, pane.id)}
				>
					<XIcon class="size-3" />
				</button>
			{/if}
		</div>
	{/each}
</div>

<style>
	/* Two dots trading places: one session changing hands. */
	.handoff {
		position: relative;
		width: 22px;
		height: 8px;
	}
	.handoff::before,
	.handoff::after {
		content: '';
		position: absolute;
		top: 0;
		width: 8px;
		height: 8px;
		border-radius: 999px;
		animation: trade 0.9s ease-in-out infinite alternate;
	}
	.handoff::before {
		left: 0;
		background: var(--agent);
	}
	.handoff::after {
		right: 0;
		background: var(--wb-ink-soft);
		animation-name: trade-back;
	}
	@keyframes trade {
		to {
			transform: translateX(14px);
		}
	}
	@keyframes trade-back {
		to {
			transform: translateX(-14px);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.handoff::before,
		.handoff::after {
			animation: none;
		}
	}
</style>
