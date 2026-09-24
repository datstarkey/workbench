<script lang="ts">
	import GitPullRequestIcon from '@lucide/svelte/icons/git-pull-request';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { cn } from '@workbench/ui';
	import { ScrollArea } from '@workbench/ui/scroll-area';
	import SidebarSection from '$features/sidebar/SidebarSection.svelte';
	import { primaryButton } from '$features/sidebar/styles';
	import { getGitHubStore, getGitStore, getWorktreeManager } from '$stores/context';
	import { openInGitHub } from '$lib/utils/github';
	import { invoke } from '@tauri-apps/api/core';
	import { onDestroy } from 'svelte';
	import { watch } from 'runed';
	import ChecksList from './ChecksList.svelte';
	import GitHubBranchBar from './GitHubBranchBar.svelte';
	import PRHeader from './PRHeader.svelte';
	import PRReadiness from './PRReadiness.svelte';
	import { checkFromPrCheck, checkFromRun, checksTone, compareUrl, TONE_BG } from './pr-view';

	const githubStore = getGitHubStore();
	const gitStore = getGitStore();
	const worktreeManager = getWorktreeManager();

	let projectPath = $derived(githubStore.sidebarTarget?.projectPath ?? null);
	let branch = $derived(githubStore.sidebarTarget?.branch ?? null);
	let pr = $derived(githubStore.sidebarPr);
	let repoUrl = $derived(projectPath ? githubStore.getRemoteUrl(projectPath) : null);
	let prChecks = $derived(githubStore.sidebarChecks.map(checkFromPrCheck));
	let runChecks = $derived((githubStore.sidebarBranchRuns?.runs ?? []).map(checkFromRun));
	let otherOpenPrs = $derived(
		projectPath
			? (githubStore.prsByProject[projectPath] ?? []).filter(
					(p) => p.state === 'OPEN' && p.headRefName !== branch
				)
			: []
	);

	async function checkoutPr() {
		if (!projectPath || !pr) return;
		await invoke('github_checkout_pr', { projectPath, prNumber: pr.number });
		await gitStore.refreshGitState(projectPath);
		await githubStore.refreshProject(projectPath);
	}

	async function openPrAsWorktree() {
		if (!projectPath || !pr) return;
		await invoke('github_fetch_pr_branch', { projectPath, branch: pr.headRefName });
		worktreeManager.add(projectPath, { suggestedBranch: pr.headRefName });
	}

	// Trigger data fetch when sidebar target changes (external side effect -- network requests)
	watch(
		() => projectPath,
		(path) => {
			if (!path) return;
			if (Date.now() - (githubStore.lastRefreshedAt[path] ?? 0) < 2000) return;
			githubStore.refreshProject(path);
		}
	);

	onDestroy(() => {
		githubStore.clearSidebarOverride();
	});
</script>

{#snippet otherPrs(path: string)}
	{#if otherOpenPrs.length > 0}
		<SidebarSection title="Other open PRs" count={otherOpenPrs.length} collapsible={false}>
			{#each otherOpenPrs as other (other.number)}
				{@const tone = checksTone(other.checksStatus.overall)}
				<button
					type="button"
					class="mx-1 flex h-[30px] items-center gap-2 rounded-[5px] px-2.5 text-left text-xs text-wb-ink hover:bg-wb-panel2"
					onclick={() => githubStore.showBranch(path, other.headRefName)}
				>
					<span class={['size-1.5 shrink-0 rounded-full', tone ? TONE_BG[tone] : 'bg-wb-hair']}
					></span>
					<span class="min-w-0 flex-1 truncate">{other.title}</span>
					<span class="shrink-0 font-mono text-[10.5px] text-wb-ink-soft">#{other.number}</span>
				</button>
			{/each}
		</SidebarSection>
	{/if}
{/snippet}

<div class="flex h-full flex-col overflow-hidden">
	<GitHubBranchBar />
	{#if projectPath && branch}
		<ScrollArea class="min-h-0 flex-1">
			{#if pr}
				<div class="flex flex-col gap-2.5 p-3">
					<PRHeader
						{pr}
						onCheckout={pr.state === 'OPEN' ? checkoutPr : undefined}
						onOpenAsWorktree={pr.state === 'OPEN' ? openPrAsWorktree : undefined}
					/>
					{#if pr.state === 'OPEN'}
						<!-- Keyed so merge options (e.g. admin bypass) never carry over to another PR -->
						{#key `${projectPath}#${pr.number}`}
							<PRReadiness {pr} {projectPath} />
						{/key}
					{/if}
				</div>
				<ChecksList
					title="Checks"
					checks={prChecks}
					{projectPath}
					emptyText={pr.checksStatus.total > 0 ? 'Loading checks…' : 'No checks on this PR'}
				/>
			{:else}
				<div class="m-3 flex flex-col gap-2.5 rounded-lg border border-dashed border-wb-hair p-3.5">
					<div class="flex items-center gap-2">
						<GitPullRequestIcon class="size-4 shrink-0 text-wb-ink-mute" />
						<span class="text-xs font-medium text-wb-ink">No pull request yet</span>
					</div>
					<p class="text-[11px] leading-[1.45] text-wb-ink-mute">
						{repoUrl
							? 'Open one on GitHub to get reviews and required checks.'
							: 'This project has no GitHub remote.'}
					</p>
					{#if repoUrl}
						<button
							type="button"
							class={cn(primaryButton, 'rounded-md')}
							onclick={() => openInGitHub(compareUrl(repoUrl, branch))}
						>
							<PlusIcon class="size-3.5" />
							Create pull request
						</button>
					{/if}
				</div>
				{#if runChecks.length > 0}
					<ChecksList
						title="Workflow runs on this branch"
						checks={runChecks}
						{projectPath}
						emptyText=""
					/>
				{/if}
			{/if}
			{@render otherPrs(projectPath)}
		</ScrollArea>
	{:else}
		<div class="flex flex-1 flex-col items-center justify-center gap-2 px-4">
			<GitPullRequestIcon class="size-5 text-wb-ink-soft" />
			<p class="text-center text-xs text-wb-ink-soft">
				Select a workspace to see its pull request and checks
			</p>
		</div>
	{/if}
</div>
