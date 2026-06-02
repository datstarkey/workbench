<script lang="ts">
	import CodeIcon from '@lucide/svelte/icons/code';
	import GithubIcon from '@lucide/svelte/icons/github';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import PanelRightOpenIcon from '@lucide/svelte/icons/panel-right-open';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { getClaudeSessionStore, getGitHubStore, getWorkspaceStore } from '$stores/context';
	import { branchUrl, openInGitHub } from '$lib/utils/github';
	import { effectivePath } from '$lib/utils/path';
	import { openInVSCode } from '$lib/utils/vscode';

	import type { ProjectWorkspace } from '$types/workbench';

	const workspaceStore = getWorkspaceStore();
	const githubStore = getGitHubStore();
	const claudeSessionStore = getClaudeSessionStore();

	let activeWorkspace = $derived(workspaceStore.activeWorkspace);

	type AttentionType = 'claude' | 'codex' | 'input' | null;

	function workspaceAttentionType(workspace: ProjectWorkspace): AttentionType {
		const sessions = claudeSessionStore.activeSessionsByProject[workspace.projectPath] ?? [];
		const wsSessions = sessions.filter(
			(s) => (s.worktreePath ?? null) === (workspace.worktreePath ?? null)
		);
		if (wsSessions.some((s) => s.awaitingInput)) return 'input';
		const attentionSessions = wsSessions.filter((s) => s.needsAttention);
		if (attentionSessions.length === 0) return null;
		return attentionSessions.some((s) => s.sessionType === 'claude') ? 'claude' : 'codex';
	}

	/** Returns the text color class for the accent bar at tab top */
	function accentBarClass(attention: AttentionType): string {
		if (attention === 'input') return 'bg-wb-err';
		if (attention === 'claude') return 'bg-wb-warn';
		if (attention === 'codex') return 'bg-wb-codex';
		return 'bg-wb-accent';
	}

	let activeGitHubUrl = $derived.by(() => {
		if (!activeWorkspace) return null;
		const repoUrl = githubStore.getRemoteUrl(activeWorkspace.projectPath);
		if (!repoUrl) return null;
		const branch = workspaceStore.resolvedBranch(activeWorkspace);
		if (branch && branch !== 'main' && branch !== 'master') {
			return branchUrl(repoUrl, branch);
		}
		return repoUrl;
	});
</script>

<!-- Tab strip: bg-wb-rail, h-[30px], border-b -->
<div class="flex h-[30px] shrink-0 items-stretch border-b border-wb-hair bg-wb-rail">
	<div class="flex flex-1 items-stretch overflow-x-auto" role="tablist" aria-label="Workspaces">
		{#each workspaceStore.workspaces as workspace (workspace.id)}
			{@const isActive = workspace.id === workspaceStore.activeWorkspaceId}
			{@const branch = workspaceStore.resolvedBranch(workspace)}
			{@const attention = workspaceAttentionType(workspace)}
			<div
				class={[
					'group relative inline-flex items-stretch border-r border-wb-hair transition-colors',
					isActive ? 'bg-wb-panel' : 'bg-transparent hover:bg-wb-panel/50'
				]}
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
				<!-- Accent underline at top -->
				{#if isActive}
					<span class={['absolute inset-x-0 top-0 h-0.5', accentBarClass(attention)]}></span>
				{/if}
				<button
					class={[
						'flex items-center gap-1.5 px-3.5 font-mono text-[11.5px] whitespace-nowrap',
						isActive ? 'text-wb-ink' : 'text-wb-ink-mute'
					]}
					type="button"
					role="tab"
					aria-selected={isActive}
					onclick={() => (workspaceStore.selectedId = workspace.id)}
				>
					{#if branch}
						<GitBranchIcon
							class={['size-3 shrink-0', isActive ? 'text-wb-accent' : 'text-wb-ink-soft']}
						/>
					{/if}
					{workspace.projectName}{#if branch}<span
							class={['ml-0.5', attention ? 'opacity-60' : 'text-wb-ink-soft']}>/{branch}</span
						>{/if}
				</button>
				<button
					class="mr-1 flex size-5 shrink-0 items-center justify-center self-center rounded text-wb-ink-soft opacity-0 transition-opacity group-hover:opacity-100 hover:bg-wb-panel2 hover:text-wb-ink"
					type="button"
					aria-label="Close project tab"
					onclick={() => workspaceStore.close(workspace.id)}
				>
					<XIcon class="size-3" />
				</button>
			</div>
		{/each}
	</div>

	<div class="flex shrink-0 items-center gap-0.5 border-l border-wb-hair px-1">
		<Tooltip.Root>
			<Tooltip.Trigger>
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-6 text-wb-ink-soft hover:bg-wb-panel2 hover:text-wb-ink"
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
						class="size-6 text-wb-ink-soft hover:bg-wb-panel2 hover:text-wb-ink"
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
						class={[
							'size-6',
							githubStore.sidebarOpen
								? 'bg-wb-panel2 text-wb-ink'
								: 'text-wb-ink-soft hover:bg-wb-panel2 hover:text-wb-ink'
						]}
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
