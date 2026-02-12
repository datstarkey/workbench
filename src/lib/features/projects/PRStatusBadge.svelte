<script lang="ts">
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import CircleXIcon from '@lucide/svelte/icons/circle-x';
	import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
	import GitPullRequestIcon from '@lucide/svelte/icons/git-pull-request';
	import GitPullRequestDraftIcon from '@lucide/svelte/icons/git-pull-request-draft';
	import GitPullRequestClosedIcon from '@lucide/svelte/icons/git-pull-request-closed';
	import GitMergeIcon from '@lucide/svelte/icons/git-merge';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import type { GitHubPR } from '$types/workbench';

	let {
		pr,
		onClickPr
	}: {
		pr: GitHubPR;
		onClickPr: () => void;
	} = $props();

	let PrIcon = $derived.by(() => {
		if (pr.state === 'MERGED') return GitMergeIcon;
		if (pr.state === 'CLOSED') return GitPullRequestClosedIcon;
		if (pr.isDraft) return GitPullRequestDraftIcon;
		return GitPullRequestIcon;
	});

	let prColor = $derived.by(() => {
		if (pr.state === 'MERGED') return 'text-purple-400';
		if (pr.state === 'CLOSED') return 'text-red-400';
		if (pr.isDraft) return 'text-muted-foreground';
		return 'text-green-400';
	});

	let ChecksIcon = $derived.by(() => {
		if (pr.checksStatus.overall === 'success') return CircleCheckIcon;
		if (pr.checksStatus.overall === 'failure') return CircleXIcon;
		if (pr.checksStatus.overall === 'pending') return LoaderCircleIcon;
		return null;
	});

	let checksColor = $derived.by(() => {
		if (pr.checksStatus.overall === 'success') return 'text-green-400';
		if (pr.checksStatus.overall === 'failure') return 'text-red-400';
		if (pr.checksStatus.overall === 'pending') return 'text-amber-400';
		return '';
	});

	let tooltipText = $derived.by(() => {
		let text = `#${pr.number}: ${pr.title}`;
		if (pr.checksStatus.overall !== 'none') {
			text += ` · CI: ${pr.checksStatus.passing}/${pr.checksStatus.total} passing`;
		}
		return text;
	});
</script>

<Tooltip.Root>
	<Tooltip.Trigger>
		<button
			class="inline-flex items-center gap-0.5 rounded px-0.5 hover:bg-muted"
			type="button"
			onclick={(e) => {
				e.stopPropagation();
				onClickPr();
			}}
		>
			<PrIcon class="size-3 shrink-0 {prColor}" />
			<span class="text-[10px] {prColor}">#{pr.number}</span>
			{#if ChecksIcon}
				<ChecksIcon
					class="size-2.5 shrink-0 {checksColor} {pr.checksStatus.overall === 'pending'
						? 'animate-spin'
						: ''}"
				/>
			{/if}
		</button>
	</Tooltip.Trigger>
	<Tooltip.Content>{tooltipText}</Tooltip.Content>
</Tooltip.Root>
