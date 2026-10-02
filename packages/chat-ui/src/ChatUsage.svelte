<script lang="ts">
	import { onMount } from 'svelte';
	import { resetCountdown, type UsageChip } from './usage-format';
	import ChatMetricPill from './ChatMetricPill.svelte';
	let { chips, chipClass }: { chips: UsageChip[]; chipClass?: string } = $props();
	let now = $state(Date.now());
	onMount(() => {
		const timer = setInterval(() => (now = Date.now()), 30_000);
		return () => clearInterval(timer);
	});
</script>

{#each chips as chip (chip.label)}
	<ChatMetricPill percent={chip.percent} label={chip.label} title={chip.title} class={chipClass}>
		{#snippet detail()}
			{@const countdown = resetCountdown(chip.resetsAt, now)}
			<div class="flex flex-col gap-2">
				<p class="font-semibold">
					{chip.label === '5h'
						? '5-hour limit'
						: chip.label === 'Week'
							? 'Weekly limit'
							: chip.label}
				</p>
				<p>{chip.percent}% used</p>
				{#if countdown}<p>{countdown}</p>{/if}
				<p class="text-wb-ink-mute">
					{#if chip.resetsAt !== null}
						Resets {new Date(chip.resetsAt * 1000).toLocaleString([], {
							weekday: 'short',
							month: 'short',
							day: 'numeric',
							hour: 'numeric',
							minute: '2-digit',
							timeZoneName: 'short'
						})}
					{:else if chip.resets}Resets {chip.resets}
					{:else}Reset time unavailable{/if}
				</p>
			</div>
		{/snippet}
	</ChatMetricPill>
{/each}
