<script lang="ts">
	import PanelLeftCloseIcon from '@lucide/svelte/icons/panel-left-close';
	import PanelLeftOpenIcon from '@lucide/svelte/icons/panel-left-open';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import { ControlPlaneSidebar } from '@workbench/control-plane-ui';
	import InstanceSwitcher from './InstanceSwitcher.svelte';
	import type { RemoteInstance } from './instances.svelte';

	let {
		instance,
		sidebarCollapsed,
		onToggleSidebar,
		onConnect
	}: {
		instance: RemoteInstance;
		sidebarCollapsed: boolean;
		onToggleSidebar: () => void;
		onConnect: () => void;
	} = $props();
</script>

<aside class="flex h-full w-full flex-col overflow-hidden border-r border-wb-hair bg-wb-panel">
	<div class="flex h-[38px] shrink-0 items-center gap-1 border-b border-wb-hair px-2.5">
		{#if !sidebarCollapsed}
			<InstanceSwitcher {onConnect} />
			<div class="ml-auto flex items-center gap-0.5">
				<button
					class="flex size-[22px] items-center justify-center rounded text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
					title="Refresh"
					onclick={() => instance.store.refresh()}
				>
					<RefreshCwIcon class="size-3" />
				</button>
				<button
					class="flex size-[22px] items-center justify-center rounded text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
					title="Collapse"
					onclick={onToggleSidebar}
				>
					<PanelLeftCloseIcon class="size-3" />
				</button>
			</div>
		{:else}
			<button
				class="mx-auto flex size-[22px] items-center justify-center rounded text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
				title="Expand"
				onclick={onToggleSidebar}
			>
				<PanelLeftOpenIcon class="size-3" />
			</button>
		{/if}
	</div>

	{#if !sidebarCollapsed}
		{#if instance.status === 'offline'}
			<div class="px-3 py-8 text-center">
				<p class="font-mono text-[11px] text-wb-err">Offline</p>
				<p class="mt-1 font-mono text-[10.5px] text-wb-ink-soft">
					Can't reach {instance.config.url}
				</p>
			</div>
		{:else if instance.status === 'connecting'}
			<p class="px-3 py-8 text-center font-mono text-[11px] text-wb-ink-soft">Connecting…</p>
		{:else}
			<div class="min-h-0 flex-1 overflow-y-auto">
				<ControlPlaneSidebar store={instance.store} />
			</div>
		{/if}
	{/if}
</aside>
