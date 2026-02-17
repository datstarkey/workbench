<script lang="ts">
	import GitPullRequestIcon from '@lucide/svelte/icons/git-pull-request';
	import GitPullRequestDraftIcon from '@lucide/svelte/icons/git-pull-request-draft';
	import GitPullRequestClosedIcon from '@lucide/svelte/icons/git-pull-request-closed';
	import GitMergeIcon from '@lucide/svelte/icons/git-merge';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import GitForkIcon from '@lucide/svelte/icons/git-fork';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import SendHorizontalIcon from '@lucide/svelte/icons/send-horizontal';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import type { GitHubCheckDetail, GitHubPR } from '$types/workbench';
	import { openInGitHub } from '$lib/utils/github';
	import { invoke } from '@tauri-apps/api/core';
	import { getGitHubStore } from '$stores/context';

	let {
		pr,
		checks,
		projectPath,
		onCheckout,
		onOpenAsWorktree
	}: {
		pr: GitHubPR;
		checks: GitHubCheckDetail[];
		projectPath: string;
		onCheckout?: () => Promise<void>;
		onOpenAsWorktree?: () => Promise<void>;
	} = $props();

	const githubStore = getGitHubStore();

	let merging = $state(false);
	let mergeError = $state<string | null>(null);
	let updating = $state(false);
	let updateError = $state<string | null>(null);
	let markingReady = $state(false);
	let readyError = $state<string | null>(null);
	let checkingOut = $state(false);
	let checkoutError = $state<string | null>(null);
	let openingWorktree = $state(false);
	let worktreeError = $state<string | null>(null);

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

	let mergeStateInfo = $derived.by(() => {
		switch (pr.mergeStateStatus) {
			case 'BEHIND':
				return {
					label: 'Behind base',
					variant: 'outline' as const,
					class: 'border-yellow-600 text-yellow-400'
				};
			case 'DIRTY':
				return { label: 'Has conflicts', variant: 'destructive' as const, class: '' };
			default:
				return null;
		}
	});

	let canMerge = $derived(
		pr.state === 'OPEN' &&
			!pr.isDraft &&
			pr.mergeStateStatus !== 'DIRTY' &&
			checks.every((c) => c.bucket !== 'fail' && c.bucket !== 'pending')
	);
	let showDraftAction = $derived(pr.isDraft && pr.state === 'OPEN');
	let showUpdateBranch = $derived(pr.mergeStateStatus === 'BEHIND' && pr.state === 'OPEN');
	let hasActionButton = $derived(canMerge || showDraftAction || showUpdateBranch);

	function runPrAction(
		setLoading: (v: boolean) => void,
		setError: (v: string | null) => void,
		command: string
	) {
		return async () => {
			setLoading(true);
			setError(null);
			try {
				await invoke(command, { projectPath, prNumber: pr.number });
				await githubStore.refreshProject(projectPath);
			} catch (e) {
				setError(String(e));
			} finally {
				setLoading(false);
			}
		};
	}

	const handleMerge = runPrAction(
		(v) => (merging = v),
		(v) => (mergeError = v),
		'github_merge_pr'
	);
	const handleUpdateBranch = runPrAction(
		(v) => (updating = v),
		(v) => (updateError = v),
		'github_update_pr_branch'
	);
	const handleMarkReady = runPrAction(
		(v) => (markingReady = v),
		(v) => (readyError = v),
		'github_mark_pr_ready'
	);

	async function handleCheckout() {
		if (!onCheckout) return;
		checkingOut = true;
		checkoutError = null;
		try {
			await onCheckout();
		} catch (e) {
			checkoutError = String(e);
		} finally {
			checkingOut = false;
		}
	}

	async function handleOpenAsWorktree() {
		if (!onOpenAsWorktree) return;
		openingWorktree = true;
		worktreeError = null;
		try {
			await onOpenAsWorktree();
		} catch (e) {
			worktreeError = String(e);
		} finally {
			openingWorktree = false;
		}
	}
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
		{#if mergeStateInfo}
			<Badge variant={mergeStateInfo.variant} class="text-[10px] {mergeStateInfo.class}"
				>{mergeStateInfo.label}</Badge
			>
		{/if}
	</div>

	{#if checksSummary}
		<p class="text-[11px] text-muted-foreground">{checksSummary}</p>
	{/if}

	<div class="flex flex-wrap gap-1.5">
		{#if onCheckout}
			<Button
				variant="outline"
				size="sm"
				class="h-7 gap-1.5 text-xs"
				onclick={handleCheckout}
				disabled={checkingOut}
			>
				{#if checkingOut}
					<LoaderIcon class="size-3 animate-spin" />
				{:else}
					<GitBranchIcon class="size-3" />
				{/if}
				Checkout
			</Button>
		{/if}
		{#if onOpenAsWorktree}
			<Button
				variant="outline"
				size="sm"
				class="h-7 gap-1.5 text-xs"
				onclick={handleOpenAsWorktree}
				disabled={openingWorktree}
			>
				{#if openingWorktree}
					<LoaderIcon class="size-3 animate-spin" />
				{:else}
					<GitForkIcon class="size-3" />
				{/if}
				Open as Worktree
			</Button>
		{/if}
		{#if showDraftAction}
			<Button
				variant="outline"
				size="sm"
				class="h-7 flex-1 gap-1.5 text-xs"
				onclick={handleMarkReady}
				disabled={markingReady}
			>
				{#if markingReady}
					<LoaderIcon class="size-3 animate-spin" />
				{:else}
					<SendHorizontalIcon class="size-3" />
				{/if}
				Ready for Review
			</Button>
		{:else if canMerge}
			<Button
				variant="default"
				size="sm"
				class="h-7 flex-1 gap-1.5 text-xs"
				onclick={handleMerge}
				disabled={merging}
			>
				{#if merging}
					<LoaderIcon class="size-3 animate-spin" />
				{:else}
					<GitMergeIcon class="size-3" />
				{/if}
				Merge
			</Button>
		{/if}
		{#if showUpdateBranch}
			<Button
				variant="outline"
				size="sm"
				class="h-7 flex-1 gap-1.5 text-xs"
				onclick={handleUpdateBranch}
				disabled={updating}
			>
				{#if updating}
					<LoaderIcon class="size-3 animate-spin" />
				{:else}
					<RefreshCwIcon class="size-3" />
				{/if}
				Update Branch
			</Button>
		{/if}
		<Button
			variant="outline"
			size="sm"
			class="h-7 gap-1.5 text-xs {hasActionButton ? '' : 'w-full'}"
			onclick={() => openInGitHub(pr.url)}
		>
			<ExternalLinkIcon class="size-3" />
			Open on GitHub
		</Button>
	</div>

	{#snippet errorMsg(msg: string | null)}
		{#if msg}
			<p class="text-[10px] text-destructive">{msg}</p>
		{/if}
	{/snippet}
	{@render errorMsg(mergeError)}
	{@render errorMsg(updateError)}
	{@render errorMsg(readyError)}
	{@render errorMsg(checkoutError)}
	{@render errorMsg(worktreeError)}
</div>
