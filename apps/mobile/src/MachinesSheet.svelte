<script lang="ts">
	import PlusIcon from '@lucide/svelte/icons/plus';
	import type { MobileClient } from './client.svelte.ts';
	import MachineList from './MachineList.svelte';
	import Sheet from './Sheet.svelte';

	/** The home screen's machine switcher: rename this one, switch, forget, add. */
	let { client, onClose }: { client: MobileClient; onClose: () => void } = $props();
</script>

<Sheet label="Machines" {onClose}>
	<div class="flex flex-col gap-4 pb-2">
		<p class="text-[12px] text-wb-ink-mute">
			Give your hosts nicknames with the pencil button. You can rename an offline host too.
		</p>
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
