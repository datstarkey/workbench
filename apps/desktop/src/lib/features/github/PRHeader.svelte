<script lang="ts">
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import GitForkIcon from '@lucide/svelte/icons/git-fork';
	import GitMergeIcon from '@lucide/svelte/icons/git-merge';
	import GitPullRequestClosedIcon from '@lucide/svelte/icons/git-pull-request-closed';
	import GitPullRequestDraftIcon from '@lucide/svelte/icons/git-pull-request-draft';
	import GitPullRequestIcon from '@lucide/svelte/icons/git-pull-request';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import { cn } from '@workbench/ui';
	import { iconButton } from '$features/sidebar/styles';
	import type { GitHubPR } from '$types/workbench';
	import { openInGitHub } from '$lib/utils/github';
	import { toast } from 'svelte-sonner';
	import { prStateLabel } from './pr-view';

	let {
		pr,
		onCheckout,
		onOpenAsWorktree
	}: {
		pr: GitHubPR;
		onCheckout?: () => Promise<void>;
		onOpenAsWorktree?: () => Promise<void>;
	} = $props();

	let busy = $state<'checkout' | 'worktree' | null>(null);
	let stateLabel = $derived(prStateLabel(pr));
	let StateIcon = $derived.by(() => {
		if (pr.state === 'MERGED') return GitMergeIcon;
		if (pr.state === 'CLOSED') return GitPullRequestClosedIcon;
		if (pr.isDraft) return GitPullRequestDraftIcon;
		return GitPullRequestIcon;
	});

	async function run(kind: 'checkout' | 'worktree', action: () => Promise<void>) {
		busy = kind;
		try {
			await action();
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = null;
		}
	}
</script>

<div class="flex flex-col gap-2">
	<div class="flex items-center gap-1.5">
		<span
			class="flex h-5 items-center gap-1 rounded-full px-[7px] text-[11px] font-semibold {stateLabel.class}"
		>
			<StateIcon class="size-[11px]" />
			{stateLabel.label}
		</span>
		<span class="font-mono text-[11px] text-wb-ink-soft">#{pr.number}</span>
		<span class="flex-1"></span>
		{#if onCheckout}
			<button
				type="button"
				class={cn(iconButton, 'size-6')}
				aria-label="Check out branch"
				title="Check out branch"
				disabled={busy !== null}
				onclick={() => run('checkout', onCheckout)}
			>
				{#if busy === 'checkout'}
					<LoaderIcon class="size-3.5 animate-spin" />
				{:else}
					<GitBranchIcon class="size-3.5" />
				{/if}
			</button>
		{/if}
		{#if onOpenAsWorktree}
			<button
				type="button"
				class={cn(iconButton, 'size-6')}
				aria-label="Open as worktree"
				title="Open as worktree"
				disabled={busy !== null}
				onclick={() => run('worktree', onOpenAsWorktree)}
			>
				{#if busy === 'worktree'}
					<LoaderIcon class="size-3.5 animate-spin" />
				{:else}
					<GitForkIcon class="size-3.5" />
				{/if}
			</button>
		{/if}
		<button
			type="button"
			class={cn(iconButton, 'size-6')}
			aria-label="Open on GitHub"
			title="Open on GitHub"
			onclick={() => openInGitHub(pr.url)}
		>
			<ExternalLinkIcon class="size-3.5" />
		</button>
	</div>
	<p class="text-[13.5px] leading-[1.35] font-semibold text-wb-ink">{pr.title}</p>
</div>
