<script lang="ts">
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import CircleXIcon from '@lucide/svelte/icons/circle-x';
	import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import { Button } from '$lib/components/ui/button';
	import type { GitHubBranchRuns } from '$types/workbench';
	import { openInGitHub, branchUrl } from '$lib/utils/github';

	let {
		branch,
		repoUrl,
		branchRuns
	}: {
		branch: string;
		repoUrl: string | null;
		branchRuns: GitHubBranchRuns;
	} = $props();

	let StatusIcon = $derived.by(() => {
		if (branchRuns.status.overall === 'success') return CircleCheckIcon;
		if (branchRuns.status.overall === 'failure') return CircleXIcon;
		if (branchRuns.status.overall === 'pending') return LoaderCircleIcon;
		return null;
	});

	let statusColor = $derived.by(() => {
		if (branchRuns.status.overall === 'success') return 'text-green-400';
		if (branchRuns.status.overall === 'failure') return 'text-red-400';
		if (branchRuns.status.overall === 'pending') return 'text-amber-400';
		return '';
	});

	let checksSummary = $derived(
		`${branchRuns.status.passing}/${branchRuns.status.total} workflows passing`
	);
</script>

<div class="space-y-2 px-3 py-2">
	<div class="flex items-start gap-2">
		<GitBranchIcon class="mt-0.5 size-4 shrink-0 text-muted-foreground" />
		<div class="min-w-0 flex-1">
			<p class="text-sm leading-snug font-medium">{branch}</p>
			<div class="flex items-center gap-1">
				{#if StatusIcon}
					<StatusIcon
						class="size-3 shrink-0 {statusColor} {branchRuns.status.overall === 'pending'
							? 'animate-spin'
							: ''}"
					/>
				{/if}
				<p class="text-xs text-muted-foreground">{checksSummary}</p>
			</div>
		</div>
	</div>

	{#if repoUrl}
		<Button
			variant="outline"
			size="sm"
			class="h-7 w-full gap-1.5 text-xs"
			onclick={() => openInGitHub(branchUrl(repoUrl!, branch))}
		>
			<ExternalLinkIcon class="size-3" />
			View Branch on GitHub
		</Button>
	{/if}
</div>
