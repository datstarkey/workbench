<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import { watch } from 'runed';
	import { ScrollArea } from '@workbench/ui/scroll-area';
	import { getGitHubStore, getGitStore, getWorkspaceStore } from '$stores/context';
	import { effectivePath } from '$lib/utils/path';
	import GitBranchHeader from './GitBranchHeader.svelte';
	import GitChanges from './GitChanges.svelte';
	import GitCommitBox from './GitCommitBox.svelte';
	import GitHistory from './GitHistory.svelte';
	import GitStashSection from './GitStashSection.svelte';
	import { stagedFiles } from './git-view';

	const gitStore = getGitStore();
	const githubStore = getGitHubStore();
	const workspaceStore = getWorkspaceStore();

	let workspace = $derived(workspaceStore.activeWorkspace);
	let activePath = $derived(workspace ? effectivePath(workspace) : null);

	let status = $derived(workspaceStore.activeGitStatus);
	let log = $derived(activePath ? (gitStore.logByProject[activePath] ?? []) : []);
	let stashes = $derived(activePath ? (gitStore.stashByProject[activePath] ?? []) : []);

	// Fetch git data when active path changes (network side effect)
	watch(
		() => activePath,
		(path) => {
			if (!path) return;
			if (Date.now() - (gitStore.lastRefreshedAt[path] ?? 0) < 2000) return;
			gitStore.refreshGitState(path);
		}
	);
</script>

{#if workspace && activePath && status}
	<ScrollArea class="h-full">
		<GitBranchHeader {status} path={activePath} projectPath={workspace.projectPath} />
		{#if status.files.length === 0}
			<div class="m-3 flex items-center gap-2.5 rounded-lg border border-dashed border-wb-hair p-3">
				<span
					class="flex size-7 shrink-0 items-center justify-center rounded-full bg-wb-panel2 text-wb-ok"
				>
					<CheckIcon class="size-3.5" />
				</span>
				<div class="flex flex-col gap-0.5">
					<span class="text-xs font-medium text-wb-ink">No uncommitted changes</span>
					<span class="text-[11px] text-wb-ink-soft">Edits in this workspace show up here.</span>
				</div>
			</div>
		{:else}
			{#key activePath}
				<GitCommitBox
					path={activePath}
					stagedCount={stagedFiles(status.files).length}
					hasUpstream={status.hasUpstream}
				/>
			{/key}
			<GitChanges files={status.files} path={activePath} />
		{/if}
		<GitHistory
			entries={log}
			path={activePath}
			repoUrl={githubStore.getRemoteUrl(workspace.projectPath)}
		/>
		{#if stashes.length > 0}
			<GitStashSection {stashes} path={activePath} />
		{/if}
	</ScrollArea>
{:else}
	<div class="flex h-full items-center justify-center p-4">
		<p class="text-center text-xs text-wb-ink-soft">Open a project to view git status</p>
	</div>
{/if}
