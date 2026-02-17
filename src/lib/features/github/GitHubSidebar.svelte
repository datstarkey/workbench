<script lang="ts">
	import GithubIcon from '@lucide/svelte/icons/github';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import GitPullRequestIcon from '@lucide/svelte/icons/git-pull-request';
	import { Separator } from '$lib/components/ui/separator';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { Badge } from '$lib/components/ui/badge';
	import { getGitHubStore } from '$stores/context';
	import type { GitHubCheckDetail } from '$types/workbench';
	import PRHeader from './PRHeader.svelte';
	import BranchRunsHeader from './BranchRunsHeader.svelte';
	import CheckItem from './CheckItem.svelte';
	import { onDestroy } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { toast } from 'svelte-sonner';

	const githubStore = getGitHubStore();

	let activeProjectPath = $derived(githubStore.sidebarTarget?.projectPath ?? null);
	let activeBranch = $derived(githubStore.sidebarTarget?.branch ?? null);
	let activePr = $derived(githubStore.sidebarPr);
	let activeBranchRuns = $derived(githubStore.sidebarBranchRuns);
	let checks = $derived(githubStore.sidebarChecks);

	// Convert workflow runs to check details for reuse with CheckItem
	type RunCheckDetail = GitHubCheckDetail & { runId: number };
	let runChecks = $derived.by((): RunCheckDetail[] => {
		if (!activeBranchRuns) return [];
		return activeBranchRuns.runs.map((run) => {
			let bucket: GitHubCheckDetail['bucket'];
			if (run.status === 'completed') {
				if (run.conclusion === 'success' || run.conclusion === 'skipped') bucket = 'pass';
				else if (run.conclusion === 'cancelled') bucket = 'cancel';
				else bucket = 'fail';
			} else {
				bucket = 'pending';
			}
			return {
				name: run.displayTitle,
				bucket,
				workflow: run.name,
				link: run.url,
				startedAt: run.createdAt,
				completedAt: run.status === 'completed' ? run.updatedAt : null,
				description: '',
				runId: run.id
			};
		});
	});

	// Other open PRs for the active project (feature 4)
	let otherOpenPrs = $derived.by(() => {
		if (!activeProjectPath) return [];
		const prs = githubStore.prsByProject[activeProjectPath] ?? [];
		const currentBranch = activeBranch;
		return prs.filter((p) => p.state === 'OPEN' && p.headRefName !== currentBranch);
	});

	// Group checks: fail -> pending -> pass -> skipping/cancel
	const BUCKET_ORDER: Record<string, number> = {
		fail: 0,
		pending: 1,
		pass: 2,
		skipping: 3,
		cancel: 4
	};
	function sortByBucket<T extends { bucket: string }>(items: T[]): T[] {
		return [...items].sort((a, b) => (BUCKET_ORDER[a.bucket] ?? 5) - (BUCKET_ORDER[b.bucket] ?? 5));
	}

	let groupedChecks = $derived(sortByBucket(checks));
	let groupedRunChecks = $derived(sortByBucket(runChecks));

	async function handleRerunWorkflow(projectPath: string, runId: number) {
		try {
			await invoke('github_rerun_workflow', { projectPath, runId });
			await githubStore.refreshProject(projectPath);
		} catch (e) {
			toast.error(`Failed to rerun workflow: ${e}`);
		}
	}

	// Trigger data fetch when sidebar target changes (external side effect -- network requests)
	$effect(() => {
		if (activeProjectPath) githubStore.refreshProject(activeProjectPath);
		if (activeProjectPath && activePr)
			githubStore.fetchPrChecks(activeProjectPath, activePr.number);
	});

	onDestroy(() => {
		githubStore.clearSidebarOverride();
	});
</script>

<div class="flex flex-1 flex-col overflow-hidden">
	{#if activePr && activeProjectPath}
		<ScrollArea class="flex-1">
			<PRHeader pr={activePr} {checks} projectPath={activeProjectPath} />

			{#if checks.length > 0}
				<Separator />
				<div class="py-1">
					{#each groupedChecks as check (check.name + check.workflow)}
						<CheckItem {check} />
					{/each}
				</div>
			{:else}
				<Separator />
				<div class="px-3 py-6 text-center">
					<p class="text-xs text-muted-foreground">Loading checks...</p>
				</div>
			{/if}

			{#if otherOpenPrs.length > 0}
				<Separator />
				<div class="px-3 py-2">
					<p class="mb-1.5 text-[10px] font-medium tracking-wider text-muted-foreground uppercase">
						Other open PRs
					</p>
					{#each otherOpenPrs as otherPr (otherPr.number)}
						<button
							class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-muted/50"
							onclick={() => githubStore.showBranch(activeProjectPath!, otherPr.headRefName)}
						>
							<GitPullRequestIcon class="size-3.5 shrink-0 text-green-400" />
							<span class="min-w-0 flex-1 truncate text-xs">{otherPr.title}</span>
							<Badge variant="secondary" class="shrink-0 text-[10px]">#{otherPr.number}</Badge>
						</button>
					{/each}
				</div>
			{/if}
		</ScrollArea>
	{:else if activeBranchRuns && activeBranch && activeProjectPath}
		<ScrollArea class="flex-1">
			<BranchRunsHeader
				branch={activeBranch}
				repoUrl={githubStore.getRemoteUrl(activeProjectPath)}
				branchRuns={activeBranchRuns}
			/>

			{#if groupedRunChecks.length > 0}
				<Separator />
				<div class="py-1">
					{#each groupedRunChecks as check (check.name + check.workflow)}
						<CheckItem
							{check}
							onRerun={check.bucket === 'fail'
								? () => handleRerunWorkflow(activeProjectPath!, check.runId)
								: undefined}
						/>
					{/each}
				</div>
			{/if}
		</ScrollArea>
	{:else}
		<div class="flex flex-1 flex-col items-center justify-center gap-2 px-4">
			{#if activeBranch}
				<GitBranchIcon class="size-5 text-muted-foreground/50" />
				<p class="text-center text-xs text-muted-foreground">
					No PR found for <span class="font-medium text-foreground">{activeBranch}</span>
				</p>
			{:else}
				<GithubIcon class="size-5 text-muted-foreground/50" />
				<p class="text-center text-xs text-muted-foreground">
					Select a workspace to view GitHub Actions
				</p>
			{/if}
		</div>
	{/if}
</div>
