<script lang="ts">
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import CircleXIcon from '@lucide/svelte/icons/circle-x';
	import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import type { GitHubChecksStatus } from '$types/workbench';

	let {
		status,
		onclick
	}: {
		status: GitHubChecksStatus;
		onclick: () => void;
	} = $props();

	let StatusIcon = $derived.by(() => {
		if (status.overall === 'success') return CircleCheckIcon;
		if (status.overall === 'failure') return CircleXIcon;
		if (status.overall === 'pending') return LoaderCircleIcon;
		return null;
	});

	let statusColor = $derived.by(() => {
		if (status.overall === 'success') return 'text-green-400';
		if (status.overall === 'failure') return 'text-red-400';
		if (status.overall === 'pending') return 'text-amber-400';
		return '';
	});

	let tooltipText = $derived(`CI: ${status.passing}/${status.total} passing`);
</script>

{#if StatusIcon}
	<Tooltip.Root>
		<Tooltip.Trigger>
			<button
				class="inline-flex items-center rounded px-0.5 hover:bg-muted"
				type="button"
				onclick={(e) => {
					e.stopPropagation();
					onclick();
				}}
			>
				<StatusIcon
					class="size-2.5 shrink-0 {statusColor} {status.overall === 'pending'
						? 'animate-spin'
						: ''}"
				/>
			</button>
		</Tooltip.Trigger>
		<Tooltip.Content>{tooltipText}</Tooltip.Content>
	</Tooltip.Root>
{/if}
