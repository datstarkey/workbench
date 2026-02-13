<script lang="ts">
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import CircleXIcon from '@lucide/svelte/icons/circle-x';
	import CircleMinusIcon from '@lucide/svelte/icons/circle-minus';
	import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import { Button } from '$lib/components/ui/button';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import type { GitHubCheckDetail } from '$types/workbench';
	import { openInGitHub } from '$lib/utils/github';

	let { check }: { check: GitHubCheckDetail } = $props();

	let StatusIcon = $derived.by(() => {
		switch (check.bucket) {
			case 'pass':
				return CircleCheckIcon;
			case 'fail':
				return CircleXIcon;
			case 'pending':
				return LoaderCircleIcon;
			default:
				return CircleMinusIcon;
		}
	});

	let statusColor = $derived.by(() => {
		switch (check.bucket) {
			case 'pass':
				return 'text-green-400';
			case 'fail':
				return 'text-red-400';
			case 'pending':
				return 'text-amber-400';
			default:
				return 'text-muted-foreground';
		}
	});

	let duration = $derived.by(() => {
		if (!check.startedAt || !check.completedAt) return null;
		const start = new Date(check.startedAt).getTime();
		const end = new Date(check.completedAt).getTime();
		if (isNaN(start) || isNaN(end) || start <= 0 || end <= 0) return null;
		const seconds = Math.round((end - start) / 1000);
		if (seconds < 0) return null;
		if (seconds < 60) return `${seconds}s`;
		const mins = Math.floor(seconds / 60);
		const secs = seconds % 60;
		return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`;
	});
</script>

<div class="group flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-muted/50">
	<StatusIcon
		class="size-3.5 shrink-0 {statusColor} {check.bucket === 'pending' ? 'animate-spin' : ''}"
	/>

	<div class="min-w-0 flex-1">
		<Tooltip.Root>
			<Tooltip.Trigger>
				<p class="truncate text-left text-xs font-medium">{check.name}</p>
			</Tooltip.Trigger>
			<Tooltip.Content side="left">{check.name}</Tooltip.Content>
		</Tooltip.Root>
		{#if check.workflow}
			<p class="truncate text-[10px] text-muted-foreground">{check.workflow}</p>
		{/if}
	</div>

	{#if duration}
		<span class="shrink-0 text-[10px] text-muted-foreground">{duration}</span>
	{/if}

	{#if check.link}
		<Button
			variant="ghost"
			size="icon-sm"
			class="size-5 shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
			onclick={() => openInGitHub(check.link)}
		>
			<ExternalLinkIcon class="size-3" />
		</Button>
	{/if}
</div>
