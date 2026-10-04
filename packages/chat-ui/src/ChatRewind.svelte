<script lang="ts">
	import HistoryIcon from '@lucide/svelte/icons/history';
	import type { AgentChat } from './agent-chat.svelte';
	import { rewindPreview } from './rewind';

	let {
		chat,
		cwd,
		onRestorePrompt
	}: {
		chat: AgentChat;
		cwd: string;
		/** A conversation rewind put the prompt back: show it in the composer. */
		onRestorePrompt: (text: string) => void;
	} = $props();

	const rewind = $derived(chat.rewind);
	const preview = $derived(rewindPreview(rewind?.files ?? null, cwd));
	const busy = $derived(rewind?.phase !== 'ready');

	async function apply(code: boolean, conversation: boolean) {
		const text = await chat.confirmRewind(code, conversation);
		if (text !== null) onRestorePrompt(text);
	}

	const primary =
		'rounded-md bg-wb-accent px-3 py-1 text-xs font-semibold text-wb-accent-ink hover:brightness-110 focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-50';
	const secondary =
		'rounded-md border border-wb-hair bg-wb-panel2 px-3 py-1 text-xs text-wb-ink hover:border-wb-ink-soft focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-50';
</script>

{#if rewind}
	<section
		class="w-full rounded-lg border border-wb-hair bg-wb-panel text-sm"
		aria-label="Rewind to before this message"
	>
		<header class="flex items-center gap-2 px-3.5 pt-3">
			<HistoryIcon class="size-4 shrink-0 text-wb-ink-mute" aria-hidden="true" />
			<span class="font-medium text-wb-ink">Rewind to before this message</span>
		</header>
		<div class="flex flex-col gap-1.5 px-3.5 pt-2 text-xs text-wb-ink-mute" aria-live="polite">
			{#if rewind.phase === 'checking'}
				<p>Checking which files changed since…</p>
			{:else}
				<p>{preview.summary}</p>
				{#if preview.files.length > 0}
					<ul class="flex flex-col gap-0.5 font-mono text-[11px] text-wb-ink">
						{#each preview.files as file (file)}
							<li class="truncate" title={file}>{file}</li>
						{/each}
						{#if preview.more > 0}
							<li class="text-wb-ink-soft">and {preview.more} more</li>
						{/if}
					</ul>
				{/if}
				<p>
					Restarting the conversation drops this message and everything after it, and puts the
					message back in the composer.
				</p>
			{/if}
			{#if rewind.error}
				<p class="text-wb-err" role="alert">{rewind.error}</p>
			{/if}
		</div>
		<div class="flex flex-wrap items-center gap-2 px-3.5 py-3">
			{#if preview.canRestore}
				<button type="button" class={primary} disabled={busy} onclick={() => apply(true, true)}>
					Restore code and conversation
				</button>
				<button type="button" class={secondary} disabled={busy} onclick={() => apply(false, true)}>
					Conversation only
				</button>
				<button type="button" class={secondary} disabled={busy} onclick={() => apply(true, false)}>
					Code only
				</button>
			{:else}
				<button type="button" class={primary} disabled={busy} onclick={() => apply(false, true)}>
					Restart conversation from here
				</button>
			{/if}
			<button
				type="button"
				class="ml-auto rounded-md px-3 py-1 text-xs text-wb-ink-mute hover:text-wb-ink focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-50"
				disabled={rewind.phase === 'working'}
				onclick={() => chat.cancelRewind()}
			>
				Cancel
			</button>
		</div>
	</section>
{/if}
