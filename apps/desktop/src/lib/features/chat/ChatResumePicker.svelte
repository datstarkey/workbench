<script lang="ts">
	import { onMount } from 'svelte';
	import XIcon from '@lucide/svelte/icons/x';
	import type { DiscoveredClaudeSession } from '$types/workbench';
	import { formatSessionDate } from '$lib/utils/format';

	let {
		load,
		onPick,
		onClose
	}: {
		/** Earlier conversations to offer, newest first. */
		load: () => Promise<DiscoveredClaudeSession[]>;
		onPick: (session: DiscoveredClaudeSession) => void;
		onClose: () => void;
	} = $props();

	let sessions = $state<DiscoveredClaudeSession[] | null>(null);
	let filter = $state('');

	const shown = $derived(
		(sessions ?? []).filter((s) => s.label.toLowerCase().includes(filter.trim().toLowerCase()))
	);

	onMount(() => {
		void load().then((list) => (sessions = list));
	});
</script>

<div
	class="absolute inset-x-0 bottom-full z-30 mb-2 flex max-h-96 flex-col overflow-hidden rounded-lg border border-wb-hair bg-wb-panel shadow-xl"
	role="dialog"
	aria-label="Resume a conversation"
>
	<header class="flex items-center gap-2 border-b border-wb-hair px-3 py-2">
		<span class="text-xs font-medium text-wb-ink">Resume a conversation</span>
		<button
			type="button"
			class="ml-auto rounded p-0.5 text-wb-ink-soft hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
			aria-label="Close"
			onclick={onClose}
		>
			<XIcon class="size-3.5" />
		</button>
	</header>
	{#if sessions && sessions.length > 6}
		<label class="border-b border-wb-hair px-3 py-1.5">
			<span class="sr-only">Filter conversations</span>
			<!-- svelte-ignore a11y_autofocus -->
			<input
				bind:value={filter}
				autofocus
				placeholder="Filter by first message"
				class="w-full bg-transparent text-xs text-wb-ink placeholder:text-wb-ink-soft focus:outline-none"
				onkeydown={(e) => e.key === 'Escape' && onClose()}
			/>
		</label>
	{/if}
	<ul class="scrollbar-thin min-h-0 flex-1 overflow-y-auto p-1">
		{#if sessions === null}
			<li class="px-2.5 py-3 text-xs text-wb-ink-soft">Looking for earlier conversations…</li>
		{:else if shown.length === 0}
			<li class="px-2.5 py-3 text-xs text-wb-ink-soft">
				{sessions.length === 0 ? 'No earlier conversations in this folder.' : 'Nothing matches.'}
			</li>
		{/if}
		{#each shown as session (session.sessionId)}
			<li>
				<button
					type="button"
					class="flex w-full items-baseline gap-3 rounded-md px-2.5 py-1.5 text-left text-xs hover:bg-wb-panel2 focus-visible:bg-wb-accent-soft focus-visible:outline-none"
					onclick={() => onPick(session)}
				>
					<span class="min-w-0 flex-1 truncate text-wb-ink">{session.label}</span>
					<span class="shrink-0 text-[11px] text-wb-ink-soft">
						{formatSessionDate(session.timestamp)}
					</span>
				</button>
			</li>
		{/each}
	</ul>
</div>
