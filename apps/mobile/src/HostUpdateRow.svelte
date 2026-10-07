<script lang="ts">
	import type { HostUpdate } from './host-update.svelte.ts';

	/** The connected host's Workbench version, with Update (after a confirm) when one is available. */
	let { update }: { update: HostUpdate } = $props();

	let confirming = $state(false);
</script>

{#if update.status}
	{@const { current, available } = update.status}
	<div class="flex flex-col gap-2 rounded-xl border border-wb-hair-soft bg-wb-panel p-3">
		<div class="flex min-h-10 items-center gap-2">
			<div class="flex min-w-0 flex-1 flex-col">
				<span class="text-[13px] font-semibold">Workbench on this host</span>
				<span class="font-mono text-[11px] text-wb-ink-soft">v{current}</span>
			</div>
			{#if update.updating}
				<span class="animate-pulse text-[12px] font-medium text-wb-warn">Updating host…</span>
			{:else if available && !confirming}
				<button
					type="button"
					class="h-10 rounded-lg bg-wb-accent px-3 text-[13px] font-semibold text-wb-accent-ink active:brightness-90"
					onclick={() => (confirming = true)}
				>
					Update to v{available}
				</button>
			{:else if !available}
				<span class="text-[12px] text-wb-ink-mute">Up to date</span>
			{/if}
		</div>
		{#if confirming && available && !update.updating}
			<p class="text-[12px] text-wb-ink-mute">
				Every terminal and chat running on this host will end, then Workbench restarts on v{available}.
				Conversations stay on disk and can be resumed.
			</p>
			<div class="flex justify-end gap-1">
				<button
					type="button"
					class="h-10 rounded-lg px-3 text-[13px] font-medium text-wb-ink-mute active:bg-wb-panel2"
					onclick={() => (confirming = false)}
				>
					Cancel
				</button>
				<button
					type="button"
					class="h-10 rounded-lg px-3 text-[13px] font-semibold text-wb-err active:bg-wb-panel2"
					onclick={() => {
						confirming = false;
						void update.install();
					}}
				>
					End sessions and update
				</button>
			</div>
		{/if}
		{#if update.error}
			<p role="alert" class="text-[12px] text-wb-err">{update.error}</p>
		{/if}
	</div>
{/if}
