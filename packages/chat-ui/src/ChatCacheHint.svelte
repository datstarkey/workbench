<script lang="ts">
	import SnowflakeIcon from '@lucide/svelte/icons/snowflake';
	import XIcon from '@lucide/svelte/icons/x';
	import type { AgentChat } from './agent-chat.svelte';
	import { cacheClock, offerCompact } from './prompt-cache';

	let { chat }: { chat: AgentChat } = $props();

	/** The expiry the hint was dismissed for; a later one shows it again. */
	let dismissed = $state<number | null>(null);
	const show = $derived(
		chat.status === 'live' &&
			offerCompact(chat.meta, cacheClock()) &&
			dismissed !== chat.meta?.cacheExpiresAt
	);
</script>

{#if show}
	<div
		class="hint flex items-center gap-2 rounded-md border border-wb-hair bg-wb-panel px-3 py-1.5 text-xs text-wb-ink-soft"
		role="status"
	>
		<SnowflakeIcon class="size-3.5 shrink-0 text-wb-warn" aria-hidden="true" />
		<span class="min-w-0 flex-1">
			Cache expired. The next message re-sends {(chat.meta?.contextTokens ?? 0).toLocaleString()} tokens.
		</span>
		<button
			type="button"
			class="shrink-0 rounded-md border border-wb-hair px-2 py-0.5 text-wb-ink hover:bg-wb-panel2 focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
			onclick={() => chat.prompt('/compact')}
		>
			Compact first
		</button>
		<button
			type="button"
			class="shrink-0 rounded-md p-0.5 text-wb-ink-mute hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
			aria-label="Dismiss"
			onclick={() => (dismissed = chat.meta?.cacheExpiresAt ?? null)}
		>
			<XIcon class="size-3.5" />
		</button>
	</div>
{/if}

<style>
	.hint {
		animation: rise 200ms ease-out;
	}
	@keyframes rise {
		from {
			opacity: 0;
			transform: translateY(3px);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.hint {
			animation: none;
		}
	}
</style>
