<script lang="ts">
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { Separator } from '$lib/components/ui/separator';
	import { getGitStore, getWorkspaceStore } from '$stores/context';
	import { effectivePath } from '$lib/utils/path';
	import GitStatusSection from './GitStatusSection.svelte';
	import GitLogSection from './GitLogSection.svelte';
	import GitBranchSection from './GitBranchSection.svelte';
	import GitStashSection from './GitStashSection.svelte';

	const gitStore = getGitStore();
	const workspaceStore = getWorkspaceStore();

	let activePath = $derived(
		workspaceStore.activeWorkspace ? effectivePath(workspaceStore.activeWorkspace) : null
	);

	let status = $derived(activePath ? gitStore.statusByProject[activePath] : undefined);
	let log = $derived(activePath ? (gitStore.logByProject[activePath] ?? []) : []);
	let stashes = $derived(activePath ? (gitStore.stashByProject[activePath] ?? []) : []);

	// Fetch git data when active path changes (network side effect)
	$effect(() => {
		if (activePath) {
			gitStore.refreshGitState(activePath);
		}
	});
</script>

{#if activePath && status}
	<ScrollArea class="h-full">
		<div class="space-y-1 py-1">
			<GitStatusSection {status} path={activePath} />
			<Separator class="opacity-40" />
			<GitLogSection entries={log} />
			<Separator class="opacity-40" />
			<GitBranchSection path={activePath} />
			<Separator class="opacity-40" />
			<GitStashSection {stashes} path={activePath} />
		</div>
	</ScrollArea>
{:else}
	<div class="flex h-full items-center justify-center p-4">
		<p class="text-center text-xs text-muted-foreground">Open a project to view git status</p>
	</div>
{/if}
