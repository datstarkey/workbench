<script lang="ts">
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import MonitorIcon from '@lucide/svelte/icons/monitor';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import XIcon from '@lucide/svelte/icons/x';
	import { getInstancesStore } from '$stores/context';
	import type { InstanceStatus } from './instances.svelte';

	let { onConnect }: { onConnect: () => void } = $props();

	const instances = getInstancesStore();

	const activeName = $derived(
		instances.activeIsLocal
			? instances.localName
			: (instances.activeRemote?.config.name ?? 'Unknown')
	);
	const activeStatus = $derived<InstanceStatus | 'local'>(
		instances.activeIsLocal ? 'local' : (instances.activeRemote?.status ?? 'connecting')
	);

	function dotClass(status: InstanceStatus | 'local'): string {
		if (status === 'online' || status === 'local') return 'bg-wb-ok';
		if (status === 'offline') return 'bg-wb-err';
		return 'bg-wb-ink-soft';
	}
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger
		class="flex min-w-0 flex-1 items-center gap-1.5 rounded px-1.5 py-1 text-left transition-colors hover:bg-wb-panel2"
	>
		{#if instances.activeIsLocal}
			<MonitorIcon class="size-3.5 shrink-0 text-wb-accent" />
		{:else}
			<span class="size-1.5 shrink-0 rounded-full {dotClass(activeStatus)}"></span>
		{/if}
		<span class="truncate text-[11px] font-semibold tracking-wide text-wb-ink-soft">
			{activeName}
		</span>
		<ChevronDownIcon class="size-3 shrink-0 text-wb-ink-mute" />
	</DropdownMenu.Trigger>

	<DropdownMenu.Content class="min-w-52" align="start">
		<DropdownMenu.Item onSelect={() => instances.setActive(instances.localId)}>
			<MonitorIcon class="size-3.5 text-wb-accent" />
			<span class="flex-1">{instances.localName}</span>
			{#if instances.activeIsLocal}<span class="text-xs text-wb-ink-soft">active</span>{/if}
		</DropdownMenu.Item>

		{#if instances.remotes.length > 0}
			<DropdownMenu.Separator />
			{#each instances.remotes as remote (remote.config.id)}
				<DropdownMenu.Item onSelect={() => instances.setActive(remote.config.id)}>
					<span class="size-1.5 rounded-full {dotClass(remote.status)}"></span>
					<span class="flex-1 truncate">{remote.config.name}</span>
					<span class="text-[10px] text-wb-ink-soft">{remote.status}</span>
					<button
						class="ml-1 rounded p-0.5 text-wb-ink-soft hover:text-wb-err"
						title="Remove"
						onclick={(e) => {
							e.stopPropagation();
							instances.remove(remote.config.id);
						}}
					>
						<XIcon class="size-3" />
					</button>
				</DropdownMenu.Item>
			{/each}
		{/if}

		<DropdownMenu.Separator />
		<DropdownMenu.Item onSelect={onConnect}>
			<PlusIcon class="size-3.5" />
			<span>Connect instance…</span>
		</DropdownMenu.Item>
	</DropdownMenu.Content>
</DropdownMenu.Root>
