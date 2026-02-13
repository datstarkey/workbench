<script lang="ts">
	import CodeIcon from '@lucide/svelte/icons/code';
	import GithubIcon from '@lucide/svelte/icons/github';
	import PanelRightOpenIcon from '@lucide/svelte/icons/panel-right-open';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { getGitHubStore, getWorkspaceStore } from '$stores/context';
	import { branchUrl, openInGitHub } from '$lib/utils/github';
	import { effectivePath } from '$lib/utils/path';
	import { openInVSCode } from '$lib/utils/vscode';

	const workspaceStore = getWorkspaceStore();
	const githubStore = getGitHubStore();

	let activeWorkspace = $derived(
		workspaceStore.workspaces.find((ws) => ws.id === workspaceStore.activeWorkspaceId)
	);

	let activeGitHubUrl = $derived.by(() => {
		if (!activeWorkspace) return null;
		const repoUrl = githubStore.getRemoteUrl(activeWorkspace.projectPath);
		if (!repoUrl) return null;
		if (
			activeWorkspace.branch &&
			activeWorkspace.branch !== 'main' &&
			activeWorkspace.branch !== 'master'
		) {
			return branchUrl(repoUrl, activeWorkspace.branch);
		}
		return repoUrl;
	});
</script>

<div class="flex shrink-0 items-start border-b border-border/60 bg-muted/30 px-1 py-1">
	<div class="flex flex-1 flex-wrap items-center gap-0.5" role="tablist" aria-label="Workspaces">
		{#each workspaceStore.workspaces as workspace (workspace.id)}
			{@const isActive = workspace.id === workspaceStore.activeWorkspaceId}
			<div
				class={`inline-flex items-center rounded-md transition-colors ${isActive ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:bg-background/50 hover:text-foreground'}`}
				draggable="true"
				role="presentation"
				ondragstart={(event) => event.dataTransfer?.setData('text/workspace-id', workspace.id)}
				ondragover={(event) => event.preventDefault()}
				ondrop={(event) => {
					event.preventDefault();
					const fromId = event.dataTransfer?.getData('text/workspace-id');
					if (fromId) workspaceStore.reorder(fromId, workspace.id);
				}}
			>
				<button
					class="px-3 py-1.5 text-xs font-medium whitespace-nowrap"
					type="button"
					role="tab"
					aria-selected={isActive}
					onclick={() => (workspaceStore.selectedId = workspace.id)}
				>
					{workspace.projectName}{#if workspace.branch}
						<span class="ml-1 text-muted-foreground">({workspace.branch})</span>
					{/if}
				</button>
				<button
					class="mr-1 flex size-5 items-center justify-center rounded opacity-50 transition-opacity hover:bg-muted hover:opacity-100"
					type="button"
					aria-label="Close project tab"
					onclick={() => workspaceStore.close(workspace.id)}
				>
					<XIcon class="size-3" />
				</button>
			</div>
		{/each}
	</div>

	<div class="flex shrink-0 items-center gap-0.5 border-l border-border/60 pr-1 pl-2">
		<Tooltip.Root>
			<Tooltip.Trigger>
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-7 text-muted-foreground hover:text-foreground"
					type="button"
					onclick={() => openInVSCode(activeWorkspace ? effectivePath(activeWorkspace) : '')}
				>
					<CodeIcon class="size-3.5" />
				</Button>
			</Tooltip.Trigger>
			<Tooltip.Content>Open in VS Code</Tooltip.Content>
		</Tooltip.Root>
		{#if activeGitHubUrl}
			<Tooltip.Root>
				<Tooltip.Trigger>
					<Button
						variant="ghost"
						size="icon-sm"
						class="size-7 text-muted-foreground hover:text-foreground"
						type="button"
						onclick={() => openInGitHub(activeGitHubUrl!)}
					>
						<GithubIcon class="size-3.5" />
					</Button>
				</Tooltip.Trigger>
				<Tooltip.Content>Open in GitHub</Tooltip.Content>
			</Tooltip.Root>
		{/if}
		{#if githubStore.ghAvailable !== false}
			<Tooltip.Root>
				<Tooltip.Trigger>
					<Button
						variant="ghost"
						size="icon-sm"
						class="size-7 {githubStore.sidebarOpen
							? 'bg-accent text-foreground'
							: 'text-muted-foreground hover:text-foreground'}"
						type="button"
						onclick={() => githubStore.toggleSidebar()}
					>
						<PanelRightOpenIcon class="size-3.5" />
					</Button>
				</Tooltip.Trigger>
				<Tooltip.Content>Show/Hide GitHub Actions</Tooltip.Content>
			</Tooltip.Root>
		{/if}
	</div>
</div>
