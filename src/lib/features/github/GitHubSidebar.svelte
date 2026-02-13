<script lang="ts">
	import GithubIcon from '@lucide/svelte/icons/github';
	import XIcon from '@lucide/svelte/icons/x';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import { Button } from '$lib/components/ui/button';
	import { Separator } from '$lib/components/ui/separator';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { getGitHubStore } from '$stores/context';
	import type { GitHubCheckDetail } from '$types/workbench';
	import PRHeader from './PRHeader.svelte';
	import BranchRunsHeader from './BranchRunsHeader.svelte';
	import CheckItem from './CheckItem.svelte';
	import { onDestroy } from 'svelte';

	let { onClose }: { onClose: () => void } = $props();

	const githubStore = getGitHubStore();

	let activeProjectPath = $derived(githubStore.sidebarTarget?.projectPath ?? null);
	let activeBranch = $derived(githubStore.sidebarTarget?.branch ?? null);
	let activePr = $derived(githubStore.sidebarPr);
	let activeBranchRuns = $derived(githubStore.sidebarBranchRuns);
	let checks = $derived(githubStore.sidebarChecks);

	// Convert workflow runs to check details for reuse with CheckItem
	let runChecks = $derived.by((): GitHubCheckDetail[] => {
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
				description: ''
			};
		});
	});

	// Group checks: fail → pending → pass → skipping/cancel
	let groupedChecks = $derived.by(() => {
		const order: Record<string, number> = { fail: 0, pending: 1, pass: 2, skipping: 3, cancel: 4 };
		return [...checks].sort((a, b) => (order[a.bucket] ?? 5) - (order[b.bucket] ?? 5));
	});

	let groupedRunChecks = $derived.by(() => {
		const order: Record<string, number> = { fail: 0, pending: 1, pass: 2, skipping: 3, cancel: 4 };
		return [...runChecks].sort((a, b) => (order[a.bucket] ?? 5) - (order[b.bucket] ?? 5));
	});

	// Trigger data fetch when sidebar target changes (external side effect — network requests)
	$effect(() => {
		if (activeProjectPath) githubStore.refreshProject(activeProjectPath);
		if (activeProjectPath && activePr)
			githubStore.fetchPrChecks(activeProjectPath, activePr.number);
	});

	onDestroy(() => {
		githubStore.clearSidebarOverride();
	});
</script>

<div class="flex h-full flex-col border-l border-border/60 bg-background">
	<!-- Header -->
	<div class="flex shrink-0 items-center justify-between border-b border-border/60 px-3 py-2">
		<div class="flex items-center gap-1.5">
			<GithubIcon class="size-3.5 text-muted-foreground" />
			<span class="text-xs font-medium">GitHub Actions</span>
		</div>
		<Button variant="ghost" size="icon-sm" class="size-6" onclick={onClose}>
			<XIcon class="size-3" />
		</Button>
	</div>

	<!-- Content -->
	{#if activePr && activeProjectPath}
		<ScrollArea class="flex-1">
			<PRHeader pr={activePr} {checks} />

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
						<CheckItem {check} />
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
