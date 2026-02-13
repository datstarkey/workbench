<script lang="ts">
	import GithubIcon from '@lucide/svelte/icons/github';
	import XIcon from '@lucide/svelte/icons/x';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import { Button } from '$lib/components/ui/button';
	import { Separator } from '$lib/components/ui/separator';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { getGitHubStore, getWorkspaceStore } from '$stores/context';
	import type { GitHubCheckDetail } from '$types/workbench';
	import PRHeader from './PRHeader.svelte';
	import CheckItem from './CheckItem.svelte';
	import { onDestroy } from 'svelte';

	let { onClose }: { onClose: () => void } = $props();

	const workspaceStore = getWorkspaceStore();
	const githubStore = getGitHubStore();

	let activeWorkspace = $derived(
		workspaceStore.workspaces.find((ws) => ws.id === workspaceStore.activeWorkspaceId)
	);

	let activeBranch = $derived(activeWorkspace?.branch ?? null);
	let activeProjectPath = $derived(activeWorkspace?.projectPath ?? null);

	let activePr = $derived.by(() => {
		if (!activeProjectPath || !activeBranch) return null;
		return githubStore.getBranchStatus(activeProjectPath, activeBranch)?.pr ?? null;
	});

	let checks = $derived.by((): GitHubCheckDetail[] => {
		if (!activeProjectPath || !activePr) return [];
		return githubStore.getPrChecks(activeProjectPath, activePr.number) ?? [];
	});

	// Group checks: fail → pending → pass → skipping/cancel
	let groupedChecks = $derived.by(() => {
		const order: Record<string, number> = { fail: 0, pending: 1, pass: 2, skipping: 3, cancel: 4 };
		return [...checks].sort((a, b) => (order[a.bucket] ?? 5) - (order[b.bucket] ?? 5));
	});

	// Trigger fetch when active PR changes
	$effect(() => {
		if (activeProjectPath && activePr) {
			githubStore.setSidebarPr(activeProjectPath, activePr.number);
		} else {
			githubStore.clearSidebarPr();
		}
	});

	onDestroy(() => {
		githubStore.clearSidebarPr();
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
