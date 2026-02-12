<script lang="ts">
	import CodeIcon from '@lucide/svelte/icons/code';
	import GithubIcon from '@lucide/svelte/icons/github';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { getGitHubStore, getWorkspaceStore } from '$stores/context';
	import { branchUrl, openInGitHub } from '$lib/utils/github';
	import { openInVSCode } from '$lib/utils/vscode';

	const workspaceStore = getWorkspaceStore();
	const githubStore = getGitHubStore();

	let activeGitHubUrl = $derived.by(() => {
		const ws = workspaceStore.workspaces.find((w) => w.id === workspaceStore.activeWorkspaceId);
		if (!ws) return null;
		const repoUrl = githubStore.getRemoteUrl(ws.projectPath);
		if (!repoUrl) return null;
		if (ws.branch && ws.branch !== 'main' && ws.branch !== 'master') {
			return branchUrl(repoUrl, ws.branch);
		}
		return repoUrl;
	});
</script>

<div class="flex h-10 shrink-0 items-center border-b border-border/60 bg-muted/30 px-1">
	<div
		class="flex flex-1 items-center gap-0.5 overflow-x-auto"
		role="tablist"
		aria-label="Workspaces"
	>
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
					class="px-3 py-1.5 text-xs font-medium"
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
					onclick={() => openInVSCode(workspaceStore.activeProjectPath ?? '')}
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
	</div>
</div>
