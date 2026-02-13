<script lang="ts">
	import GithubIcon from '@lucide/svelte/icons/github';
	import XIcon from '@lucide/svelte/icons/x';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import { Button } from '$lib/components/ui/button';
	import { Separator } from '$lib/components/ui/separator';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { getGitHubStore, getGitStore, getWorkspaceStore } from '$stores/context';
	import type { GitHubCheckDetail } from '$types/workbench';
	import PRHeader from './PRHeader.svelte';
	import BranchRunsHeader from './BranchRunsHeader.svelte';
	import CheckItem from './CheckItem.svelte';
	import { onDestroy } from 'svelte';

	let { onClose }: { onClose: () => void } = $props();

	const workspaceStore = getWorkspaceStore();
	const gitStore = getGitStore();
	const githubStore = getGitHubStore();

	let activeWorkspace = $derived(
		workspaceStore.workspaces.find((ws) => ws.id === workspaceStore.activeWorkspaceId)
	);

	// Resolve project path and branch: prefer sidebarBranchKey, then active workspace
	let sidebarTarget = $derived.by(() => {
		// If user clicked a CIStatusBadge, use that
		if (githubStore.sidebarBranchKey) {
			const [projectPath, ...branchParts] = githubStore.sidebarBranchKey.split('::');
			const branch = branchParts.join('::'); // branch names can't contain :: but be safe
			if (projectPath && branch) return { projectPath, branch };
		}
		// If user clicked a PR badge, derive from sidebarPrKey
		if (githubStore.sidebarPrKey) {
			const [projectPath, prNumStr] = githubStore.sidebarPrKey.split('::');
			if (projectPath) {
				const prs = githubStore.prsByProject[projectPath] ?? [];
				const pr = prs.find((p) => p.number === parseInt(prNumStr, 10));
				if (pr) return { projectPath, branch: pr.headRefName };
			}
		}
		// Fall back to active workspace
		if (!activeWorkspace) return null;
		const projectPath = activeWorkspace.projectPath;
		// worktree workspaces have .branch set; main workspaces use gitStore
		const branch = activeWorkspace.branch ?? gitStore.branchByProject[projectPath] ?? null;
		if (!branch) return null;
		return { projectPath, branch };
	});

	let activeProjectPath = $derived(sidebarTarget?.projectPath ?? null);
	let activeBranch = $derived(sidebarTarget?.branch ?? null);

	let branchStatus = $derived.by(() => {
		if (!activeProjectPath || !activeBranch) return undefined;
		return githubStore.getBranchStatus(activeProjectPath, activeBranch);
	});

	let activePr = $derived(branchStatus?.pr ?? null);
	let activeBranchRuns = $derived(branchStatus?.branchRuns ?? null);

	let checks = $derived.by((): GitHubCheckDetail[] => {
		if (!activeProjectPath || !activePr) return [];
		return githubStore.getPrChecks(activeProjectPath, activePr.number) ?? [];
	});

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

	// Trigger fetch when active PR / branch changes
	$effect(() => {
		if (activeProjectPath && activePr) {
			githubStore.setSidebarPr(activeProjectPath, activePr.number);
		} else if (activeProjectPath && activeBranch && activeBranchRuns) {
			githubStore.setSidebarBranch(activeProjectPath, activeBranch);
		} else if (!githubStore.sidebarBranchKey && !githubStore.sidebarPrKey) {
			// Only clear if nothing was explicitly set by a badge click
			githubStore.clearSidebar();
		}
	});

	onDestroy(() => {
		githubStore.clearSidebar();
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
