<script lang="ts">
	import type { Component } from 'svelte';
	import CodeIcon from '@lucide/svelte/icons/code';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import GithubIcon from '@lucide/svelte/icons/github';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import PlayCircleIcon from '@lucide/svelte/icons/play-circle';
	import PlayIcon from '@lucide/svelte/icons/play';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import XIcon from '@lucide/svelte/icons/x';
	import ZapIcon from '@lucide/svelte/icons/zap';
	import {
		getClaudeSessionStore,
		getGitHubStore,
		getGitStore,
		getProjectManager,
		getProjectStore,
		getWorkbenchSettingsStore,
		getWorktreeManager,
		getWorkspaceStore
	} from '$stores/context';
	import { openInGitHub } from '$lib/utils/github';
	import { openInVSCode } from '$lib/utils/vscode';
	import type { AgentAction, ProjectConfig, ProjectTask } from '$types/workbench';

	let {
		project,
		tasks,
		Item,
		Separator,
		Group,
		GroupHeading,
		Sub,
		SubTrigger,
		SubContent
	}: {
		project: ProjectConfig;
		tasks: ProjectTask[];
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		Item: Component<any>;
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		Separator: Component<any>;
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		Group: Component<any>;
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		GroupHeading: Component<any>;
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		Sub: Component<any>;
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		SubTrigger: Component<any>;
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		SubContent: Component<any>;
	} = $props();

	const projectStore = getProjectStore();
	const workspaceStore = getWorkspaceStore();
	const claudeSessionStore = getClaudeSessionStore();
	const gitStore = getGitStore();
	const githubStore = getGitHubStore();
	const projectManager = getProjectManager();
	const worktreeManager = getWorktreeManager();
	const workbenchSettingsStore = getWorkbenchSettingsStore();

	function runTask(task: ProjectTask): void {
		projectStore.openProject(project.path);
		workspaceStore.runTaskByProject(project.path, task);
	}

	let runnableActions = $derived(workbenchSettingsStore.runnableActions);

	function runAction(action: AgentAction, type: 'claude' | 'codex'): void {
		claudeSessionStore.startAgentActionByProject(project.path, action, type);
	}
</script>

<Item onclick={() => projectStore.openProject(project.path)}>
	<ExternalLinkIcon class="size-3.5" />
	Open
</Item>
<Item onclick={() => claudeSessionStore.startSessionByProject(project.path)}>
	<SparklesIcon class="size-3.5" />
	New Claude Session
</Item>
{#if runnableActions.length > 0}
	<Sub>
		<SubTrigger>
			<PlayCircleIcon class="size-3.5" />
			Run Agent Action
		</SubTrigger>
		<SubContent class="w-56">
			{#each runnableActions as action (action.id)}
				{#if action.target !== 'codex'}
					<Item onclick={() => runAction(action, 'claude')}>
						<SparklesIcon class="size-3.5 text-violet-400" />
						<span class="truncate">Claude: {action.name}</span>
					</Item>
				{/if}
				{#if action.target !== 'claude'}
					<Item onclick={() => runAction(action, 'codex')}>
						<ZapIcon class="size-3.5 text-sky-400" />
						<span class="truncate">Codex: {action.name}</span>
					</Item>
				{/if}
			{/each}
		</SubContent>
	</Sub>
{/if}
<Item onclick={() => worktreeManager.add(project.path)}>
	<GitBranchIcon class="size-3.5" />
	Add Worktree
</Item>
<Sub>
	<SubTrigger>
		<FolderIcon class="size-3.5" />
		Move to Group
	</SubTrigger>
	<SubContent class="w-44">
		{#each projectStore.groupNames as groupName (groupName)}
			{#if groupName !== project.group}
				<Item onclick={() => projectStore.setGroup(project.path, groupName)}>
					{groupName}
				</Item>
			{/if}
		{/each}
		<Separator />
		<Item onclick={() => projectManager.promptNewGroup(project.path)}>
			<PlusIcon class="size-3.5" />
			New Group...
		</Item>
		{#if project.group}
			<Separator />
			<Item onclick={() => projectStore.setGroup(project.path, undefined)}>
				<XIcon class="size-3.5" />
				Remove from Group
			</Item>
		{/if}
	</SubContent>
</Sub>
<Item onclick={() => gitStore.refreshGitState(project.path)}>
	<RefreshCwIcon class="size-3.5" />
	Refresh Git
</Item>
<Item onclick={() => openInVSCode(project.path)}>
	<CodeIcon class="size-3.5" />
	Open in VS Code
</Item>
{#if githubStore.getRemoteUrl(project.path)}
	<Item onclick={() => openInGitHub(githubStore.getRemoteUrl(project.path)!)}>
		<GithubIcon class="size-3.5" />
		Open in GitHub
	</Item>
{/if}
{#if tasks.length > 0}
	<Separator />
	<Group>
		<GroupHeading>Tasks</GroupHeading>
		{#each tasks as task, i (`${task.name}-${i}`)}
			<Item onclick={() => runTask(task)}>
				<PlayIcon class="size-3.5" />
				{task.name}
			</Item>
		{/each}
	</Group>
{/if}
<Separator />
<Item onclick={() => projectManager.edit(project.path)}>
	<PencilIcon class="size-3.5" />
	Edit
</Item>
<Item class="text-destructive" onclick={() => projectManager.remove(project.path)}>
	<Trash2Icon class="size-3.5" />
	Remove
</Item>
