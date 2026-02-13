<script lang="ts">
	import GitPullRequestIcon from '@lucide/svelte/icons/git-pull-request';
	import GitPullRequestDraftIcon from '@lucide/svelte/icons/git-pull-request-draft';
	import GitPullRequestClosedIcon from '@lucide/svelte/icons/git-pull-request-closed';
	import GitMergeIcon from '@lucide/svelte/icons/git-merge';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import type { GitHubCheckDetail, GitHubPR } from '$types/workbench';
	import { openInGitHub } from '$lib/utils/github';

	let {
		pr,
		checks
	}: {
		pr: GitHubPR;
		checks: GitHubCheckDetail[];
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

	let stateLabel = $derived.by(() => {
		if (pr.state === 'MERGED') return 'Merged';
		if (pr.state === 'CLOSED') return 'Closed';
		if (pr.isDraft) return 'Draft';
		return 'Open';
	});

	let stateBadgeVariant = $derived.by((): 'default' | 'secondary' | 'destructive' | 'outline' => {
		if (pr.state === 'MERGED') return 'default';
		if (pr.state === 'CLOSED') return 'destructive';
		return 'secondary';
	});

	let reviewLabel = $derived.by(() => {
		if (!pr.reviewDecision) return null;
		switch (pr.reviewDecision) {
			case 'APPROVED':
				return 'Approved';
			case 'CHANGES_REQUESTED':
				return 'Changes Requested';
			case 'REVIEW_REQUIRED':
				return 'Review Required';
			default:
				return null;
		}
	});

	let reviewBadgeVariant = $derived.by((): 'default' | 'secondary' | 'destructive' | 'outline' => {
		if (pr.reviewDecision === 'APPROVED') return 'default';
		if (pr.reviewDecision === 'CHANGES_REQUESTED') return 'destructive';
		return 'outline';
	});

	let checksSummary = $derived.by(() => {
		if (checks.length === 0) return null;
		const passing = checks.filter((c) => c.bucket === 'pass').length;
		return `${passing}/${checks.length} checks passing`;
	});
</script>

<div class="space-y-2 px-3 py-2">
	<div class="flex items-start gap-2">
		<PrIcon class="mt-0.5 size-4 shrink-0 {prColor}" />
		<div class="min-w-0 flex-1">
			<p class="text-sm leading-snug font-medium">{pr.title}</p>
			<p class="text-xs text-muted-foreground">#{pr.number}</p>
		</div>
	</div>

	<div class="flex flex-wrap items-center gap-1.5">
		<Badge variant={stateBadgeVariant} class="text-[10px]">{stateLabel}</Badge>
		{#if reviewLabel}
			<Badge variant={reviewBadgeVariant} class="text-[10px]">{reviewLabel}</Badge>
		{/if}
	</div>

	{#if checksSummary}
		<p class="text-[11px] text-muted-foreground">{checksSummary}</p>
	{/if}

	<Button
		variant="outline"
		size="sm"
		class="h-7 w-full gap-1.5 text-xs"
		onclick={() => openInGitHub(pr.url)}
	>
		<ExternalLinkIcon class="size-3" />
		Open on GitHub
	</Button>
</div>
