<script lang="ts">
	import FlameIcon from '@lucide/svelte/icons/flame';
	import SnowflakeIcon from '@lucide/svelte/icons/snowflake';
	import { cn } from '@workbench/ui';
	import { Switch } from '@workbench/ui/switch';
	import type { AgentChat } from './agent-chat.svelte';
	import ChatMetricPill from './ChatMetricPill.svelte';
	import {
		cacheClock,
		cacheState,
		formatRemaining,
		KEEP_WARM_HOURS,
		MIN_COMPACT_TOKENS
	} from './prompt-cache';

	let { chat, class: className }: { chat: AgentChat; class?: string } = $props();

	const now = $derived(cacheClock());
	const cache = $derived(cacheState(chat.meta, now));
	const policy = $derived(chat.cachePolicy);
	const keptWarm = $derived((policy.keepWarmUntil ?? 0) > now);
	const idle = $derived(chat.status === 'live' && !chat.meta?.busy);

	const time = (ms: number) =>
		new Date(ms).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });

	function keepWarm(hours: number | null) {
		const { keepWarmUntil: _, ...rest } = policy;
		chat.setCachePolicy(hours ? { ...rest, keepWarmUntil: Date.now() + hours * 3_600_000 } : rest);
	}
</script>

{#if cache}
	<ChatMetricPill
		percent={cache.percent}
		value={cache.warm ? formatRemaining(cache.remainingMs) : 'Cold'}
		warn={!cache.warm || cache.low}
		title={cache.warm
			? `Prompt cache warm for ${formatRemaining(cache.remainingMs)}`
			: 'Prompt cache expired'}
		class={className}
	>
		{#if cache.warm}
			<FlameIcon class="size-3.5" aria-hidden="true" />
		{:else}
			<SnowflakeIcon class="size-3.5" aria-hidden="true" />
		{/if}
		{#snippet detail()}
			<div class="flex flex-col gap-3">
				<div class="flex flex-col gap-1">
					<p class="font-semibold">Prompt cache</p>
					{#if cache.warm}
						<p>Warm for {formatRemaining(cache.remainingMs)}, until {time(cache.expiresAt)}</p>
					{:else}
						<p>
							Expired at {time(cache.expiresAt)}. The next message re-sends {(
								chat.meta?.contextTokens ?? 0
							).toLocaleString()} tokens.
						</p>
					{/if}
					<p class="text-wb-ink-mute">
						{cache.ttlSecs >= 3600 ? '1-hour' : '5-minute'} cache. Every message refreshes it.
					</p>
				</div>
				<button
					type="button"
					class="rounded-md border border-wb-hair px-2 py-1 hover:bg-wb-panel2 focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none disabled:opacity-50"
					disabled={!idle || !cache.warm}
					onclick={() => chat.pingCache()}
				>
					Refresh now
				</button>
				<div class="flex flex-col gap-1.5">
					<p class="font-medium">Keep warm</p>
					<div class="flex gap-1" role="group" aria-label="Keep the cache warm">
						{#each [null, ...KEEP_WARM_HOURS] as hours (hours)}
							<button
								type="button"
								aria-pressed={hours === null ? !keptWarm : false}
								class={cn(
									'flex-1 rounded-md border border-wb-hair px-1.5 py-0.5 hover:bg-wb-panel2 focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none',
									hours === null && !keptWarm && 'border-wb-accent text-wb-accent'
								)}
								onclick={() => keepWarm(hours)}
							>
								{hours === null ? 'Off' : `${hours}h`}
							</button>
						{/each}
					</div>
					<p class="text-wb-ink-mute">
						{#if keptWarm && policy.keepWarmUntil}
							Kept warm until {time(policy.keepWarmUntil)}. Each refresh uses a little plan usage.
						{:else}
							Refreshes the cache just before it expires.
						{/if}
					</p>
				</div>
				<label class="flex items-start justify-between gap-3">
					<span class="flex flex-col gap-0.5">
						<span class="font-medium">Compact before it expires</span>
						<span class="text-wb-ink-mute">
							Over {MIN_COMPACT_TOKENS / 1000}k tokens, once keep-warm ends.
						</span>
					</span>
					<Switch
						checked={policy.compactOnExpiry}
						onCheckedChange={(compactOnExpiry) =>
							chat.setCachePolicy({ ...policy, compactOnExpiry })}
					/>
				</label>
			</div>
		{/snippet}
	</ChatMetricPill>
{/if}
