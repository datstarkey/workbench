<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import XIcon from '@lucide/svelte/icons/x';
	import { cn } from '@workbench/ui';
	import type { MobileClient } from './client.svelte.ts';

	/** Saved machines: tap one to switch to it, × to forget it. `onPick` runs before switching. */
	let { client, onPick }: { client: MobileClient; onPick?: () => void } = $props();
</script>

<ul class="flex flex-col gap-1.5">
	{#each client.machines.list as m (m.id)}
		{@const current = m.id === client.machineId}
		<li
			class={cn(
				'flex items-center gap-1 rounded-xl border bg-wb-panel py-1 pr-1 pl-3',
				current ? 'border-wb-accent/50' : 'border-wb-hair-soft'
			)}
		>
			<button
				type="button"
				class="flex min-h-11 min-w-0 flex-1 flex-col justify-center text-left disabled:opacity-50"
				aria-current={current}
				disabled={client.connecting}
				onclick={() => {
					onPick?.();
					if (!current) void client.switchTo(m.id);
				}}
			>
				<span class="flex items-center gap-1.5 text-[13.5px] font-medium">
					<span class="truncate">{m.name}</span>
					{#if current}<CheckIcon class="size-3.5 shrink-0 text-wb-accent" />{/if}
				</span>
				<span class="truncate font-mono text-[11px] text-wb-ink-soft">{m.url}</span>
			</button>
			<button
				type="button"
				class="grid size-10 shrink-0 place-items-center rounded-lg text-wb-ink-soft active:bg-wb-panel2 active:text-wb-err"
				aria-label="Forget {m.name}"
				onclick={() => client.forget(m.id)}
			>
				<XIcon class="size-4" />
			</button>
		</li>
	{/each}
</ul>
