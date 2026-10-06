<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import XIcon from '@lucide/svelte/icons/x';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import { cn } from '@workbench/ui';
	import type { MobileClient } from './client.svelte.ts';

	/** Saved machines: tap one to switch to it, × then Forget to remove it. `onPick` runs before switching. */
	let { client, onPick }: { client: MobileClient; onPick?: () => void } = $props();

	/** The machine whose × was tapped; its row asks before forgetting. */
	let confirming = $state<string | null>(null);
	let editing = $state<string | null>(null);
	let nickname = $state('');
</script>

<ul class="flex flex-col gap-1.5">
	{#each client.machines.list as m (m.id)}
		{@const current = m.id === client.machineId}
		<li
			class={cn(
				'flex min-h-13 items-center gap-1 rounded-xl border bg-wb-panel py-1 pr-1 pl-3',
				current ? 'border-wb-accent/50' : 'border-wb-hair-soft'
			)}
		>
			{#if editing === m.id}
				<form
					class="flex min-w-0 flex-1 items-center gap-1"
					onsubmit={(event) => {
						event.preventDefault();
						client.machines.rename(m.id, nickname);
						editing = null;
					}}
				>
					<input
						aria-label="Nickname for {m.name}"
						bind:value={nickname}
						{@attach (node) => node.focus()}
						onkeydown={(event) => {
							if (event.key === 'Escape') {
								event.preventDefault();
								editing = null;
							}
						}}
						autocapitalize="words"
						class="h-10 min-w-0 flex-1 rounded-lg border border-wb-hair bg-wb-panel2 px-2 text-[13px] focus:border-wb-accent focus:outline-none"
					/>
					<button
						type="submit"
						disabled={!nickname.trim()}
						class="h-10 rounded-lg px-2 text-[13px] font-semibold text-wb-accent disabled:opacity-40"
						>Save</button
					>
					<button
						type="button"
						class="h-10 rounded-lg px-2 text-[13px] text-wb-ink-mute"
						onclick={() => (editing = null)}>Cancel</button
					>
				</form>
			{:else if confirming === m.id}
				<span class="min-w-0 flex-1 truncate text-[13px]">Forget {m.name}?</span>
				<button
					type="button"
					class="h-10 rounded-lg px-3 text-[13px] font-semibold text-wb-err active:bg-wb-panel2"
					onclick={() => {
						confirming = null;
						client.forget(m.id);
					}}
				>
					Forget
				</button>
				<button
					type="button"
					class="h-10 rounded-lg px-3 text-[13px] font-medium text-wb-ink-mute active:bg-wb-panel2"
					onclick={() => (confirming = null)}
				>
					Cancel
				</button>
			{:else}
				<button
					type="button"
					class="flex min-h-11 min-w-0 flex-1 flex-col justify-center text-left"
					aria-current={current}
					onclick={() => {
						onPick?.();
						if (!current) void client.switchTo(m.id);
					}}
				>
					<span class="flex items-center gap-1.5 text-[13.5px] font-medium">
						<span class="truncate">{m.name}</span>
						{#if current}<CheckIcon class="size-3.5 shrink-0 text-wb-accent" />{/if}
					</span>
					<span class="truncate font-mono text-[11px] text-wb-ink-soft">
						{client.connectingTo === m.id ? 'Connecting…' : m.url}
					</span>
				</button>
				<button
					type="button"
					class="grid size-10 shrink-0 place-items-center rounded-lg text-wb-ink-soft active:bg-wb-panel2 active:text-wb-ink"
					aria-label="Rename {m.name}"
					onclick={() => {
						nickname = m.name;
						editing = m.id;
						confirming = null;
					}}><PencilIcon class="size-4" /></button
				>
				<button
					type="button"
					class="grid size-10 shrink-0 place-items-center rounded-lg text-wb-ink-soft active:bg-wb-panel2 active:text-wb-err"
					aria-label="Forget {m.name}"
					onclick={() => (confirming = m.id)}
				>
					<XIcon class="size-4" />
				</button>
			{/if}
		</li>
	{/each}
</ul>
