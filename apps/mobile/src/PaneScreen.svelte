<script lang="ts">
	import ChatScreen from './ChatScreen.svelte';
	import type { MobileClient } from './client.svelte.ts';
	import type { PaneEntry } from './panes.ts';
	import { paneTitle } from './panes.ts';
	import PaneWaiting from './PaneWaiting.svelte';
	import Terminal from './Terminal.svelte';

	/** One pane of the host's model: its terminal, its chat, or why neither is up yet. */
	let { client, entry }: { client: MobileClient; entry: PaneEntry } = $props();

	const pane = $derived(entry.pane);
	const view = $derived(client.paneView(pane));
	const chatReady = $derived(pane.status === 'running' && !!pane.sessionId);
</script>

{#if view === 'terminal' && pane.terminalId}
	{#key pane.terminalId}
		<Terminal
			serverUrl={client.connection?.url ?? ''}
			token={client.connection?.token ?? ''}
			id={pane.terminalId}
			name={paneTitle(entry)}
			onClose={client.closeScreen}
			onShowChat={pane.kind === 'claude' ? () => client.setView(pane.id, 'chat') : undefined}
			notice={client.notice}
		/>
	{/key}
{:else if view === 'chat' && chatReady}
	{#key client.chatScreenKey}
		<ChatScreen {client} {entry} />
	{/key}
{:else}
	<PaneWaiting {client} {entry} />
{/if}
