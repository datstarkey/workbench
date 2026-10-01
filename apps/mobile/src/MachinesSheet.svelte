<script lang="ts">
	import PlusIcon from '@lucide/svelte/icons/plus';
	import type { MobileClient } from './client.svelte.ts';
	import MachineList from './MachineList.svelte';
	import Sheet from './Sheet.svelte';

	/** The home screen's machine switcher: rename this one, switch, forget, add. */
	let { client, onClose }: { client: MobileClient; onClose: () => void } = $props();

	/** The rename field; a rejected (blank) name snaps back to the stored one. */
	let nameDraft = $derived(client.machine?.name ?? '');
</script>

<Sheet label="Machines" {onClose}>
	<div class="flex flex-col gap-4 pb-2">
		{#if client.machine}
			{@const id = client.machine.id}
			<label class="flex flex-col gap-1.5">
				<span class="text-[13px] font-semibold">This machine's name</span>
				<input
					bind:value={nameDraft}
					onchange={() => (nameDraft = client.machines.rename(id, nameDraft))}
					autocapitalize="words"
					autocorrect="off"
					spellcheck={false}
					class="h-10 rounded-lg border border-wb-hair bg-wb-panel2 px-3 text-[13px] text-wb-ink focus:border-wb-ink-soft focus:outline-none"
				/>
			</label>
		{/if}
		<div class="flex flex-col gap-2">
			<span class="text-[13px] font-semibold">Switch to</span>
			<MachineList {client} onPick={onClose} />
		</div>
		<button
			type="button"
			class="flex h-10 items-center justify-center gap-1.5 rounded-lg border border-wb-hair bg-wb-panel2 text-[13px] font-medium active:bg-wb-panel"
			onclick={() => {
				onClose();
				client.addMachine();
			}}
		>
			<PlusIcon class="size-4" />
			Add machine
		</button>
	</div>
</Sheet>
