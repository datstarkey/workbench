<script lang="ts">
	import type { Component } from 'svelte';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import CirclePauseIcon from '@lucide/svelte/icons/circle-pause';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import EllipsisVerticalIcon from '@lucide/svelte/icons/ellipsis-vertical';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import PanelLeftOpenIcon from '@lucide/svelte/icons/panel-left-open';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import SearchIcon from '@lucide/svelte/icons/search';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import XIcon from '@lucide/svelte/icons/x';
	import InstanceSwitcher from '$features/instances/InstanceSwitcher.svelte';
	import * as ContextMenu from '@workbench/ui/context-menu';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import { Input } from '@workbench/ui/input';
	import { ScrollArea } from '@workbench/ui/scroll-area';
	import { SvelteMap, SvelteSet } from 'svelte/reactivity';
	import {
		getClaudeSessionStore,
		getGitHubStore,
		getGitStore,
		getProjectManager,
		getProjectStore,
		getWorktreeManager,
		getWorkspaceStore
	} from '$stores/context';
	import { openInGitHub } from '$lib/utils/github';
	import type { ActiveClaudeSession, ProjectConfig, WorktreeInfo } from '$types/workbench';
	import type { ProjectGroup } from '$stores/projects.svelte';
	import CIStatusBadge from './CIStatusBadge.svelte';
	import PRStatusBadge from './PRStatusBadge.svelte';
	import ProjectMenuItems from './ProjectMenuItems.svelte';
	import SessionItem from './SessionItem.svelte';
	import CloneRepoDialog from './CloneRepoDialog.svelte';

	const projectStore = getProjectStore();
	const workspaceStore = getWorkspaceStore();
	const claudeSessionStore = getClaudeSessionStore();
	const gitStore = getGitStore();
	const githubStore = getGitHubStore();
	const projectManager = getProjectManager();
	const worktreeManager = getWorktreeManager();

	let {
		sidebarCollapsed,
		onToggleSidebar,
		onConnect
	}: {
		sidebarCollapsed: boolean;
		onToggleSidebar: () => void;
		onConnect: () => void;
	} = $props();

	// Explicit per-project toggles; projects the user never toggled follow "is active".
	const expandedProjects = new SvelteMap<string, boolean>();
	const collapsedGroups = new SvelteSet<string>();

	let filterText = $state('');
	let cloneDialogOpen = $state(false);
	let dragOverProjectPath = $state<string | null>(null);

	let filteredGroupedProjects: ProjectGroup[] = $derived.by(() => {
		const query = filterText.trim().toLowerCase();
		if (!query) return projectStore.groupedProjects;

		const result: ProjectGroup[] = [];
		for (const section of projectStore.groupedProjects) {
			const groupMatches = section.group?.toLowerCase().includes(query) ?? false;
			if (groupMatches) {
				result.push(section);
			} else {
				const filtered = section.projects.filter((p) => p.name.toLowerCase().includes(query));
				if (filtered.length > 0) {
					result.push({ group: section.group, projects: filtered });
				}
			}
		}
		return result;
	});

	function toggleSet<T>(set: SvelteSet<T>, value: T) {
		if (set.has(value)) set.delete(value);
		else set.add(value);
	}

	// Landing back on the default drops the override, so the project follows "is active" again.
	function toggleExpanded(path: string, expanded: boolean, isActive: boolean) {
		if (expanded === isActive) expandedProjects.delete(path);
		else expandedProjects.set(path, expanded);
	}

	function allSessionsForProject(projectPath: string): ActiveClaudeSession[] {
		return claudeSessionStore.activeSessionsByProject[projectPath] ?? [];
	}

	function worktreesForProject(projectPath: string): WorktreeInfo[] {
		return (gitStore.worktreesByProject[projectPath] ?? []).filter((wt) => !wt.isMain);
	}

	function projectAttentionType(projectPath: string): 'claude' | 'codex' | 'input' | null {
		const sessions = allSessionsForProject(projectPath);
		if (sessions.some((s) => s.awaitingInput)) return 'input';
		const attentionSessions = sessions.filter((s) => s.needsAttention);
		if (attentionSessions.length === 0) return null;
		return attentionSessions.some((s) => s.sessionType === 'claude') ? 'claude' : 'codex';
	}

	function startSessionInWorktree(projectPath: string, worktreePath: string, branch: string): void {
		worktreeManager.open(projectPath, worktreePath, branch);
		const ws = workspaceStore.getByWorktreePath(worktreePath);
		if (ws) claudeSessionStore.startSessionInWorkspace(ws);
	}

	function sessionCount(sessions: ActiveClaudeSession[], type: 'claude' | 'codex'): number {
		return sessions.filter((s) => s.sessionType === type).length;
	}
</script>

<aside class="flex h-full w-full flex-col overflow-hidden border-r border-wb-hair bg-wb-panel">
	<div class="flex h-[38px] shrink-0 items-center border-b border-wb-hair pr-2 pl-3">
		{#if sidebarCollapsed}
			<button
				class="mx-auto flex size-[22px] items-center justify-center rounded text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
				type="button"
				aria-label="Expand sidebar"
				onclick={onToggleSidebar}
			>
				<PanelLeftOpenIcon class="size-3" />
			</button>
		{:else}
			<InstanceSwitcher {onConnect} />
			<DropdownMenu.Root>
				<DropdownMenu.Trigger
					class="ml-auto flex h-6 items-center gap-1 rounded-md border border-wb-hair px-2 font-mono text-[11px] text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
				>
					<PlusIcon class="size-3" />
					Add
				</DropdownMenu.Trigger>
				<DropdownMenu.Content align="end" class="w-48">
					<DropdownMenu.Item onclick={() => projectManager.add()}>
						<FolderPlusIcon class="size-3.5" />
						Add Folder
					</DropdownMenu.Item>
					<DropdownMenu.Item onclick={() => (cloneDialogOpen = true)}>
						<DownloadIcon class="size-3.5" />
						Clone from GitHub
					</DropdownMenu.Item>
				</DropdownMenu.Content>
			</DropdownMenu.Root>
		{/if}
	</div>

	{#if !sidebarCollapsed}
		{#if projectStore.projects.length > 0}
			<div class="relative shrink-0 px-2 pt-2 pb-1">
				<SearchIcon
					class="pointer-events-none absolute top-[calc(50%+2px)] left-4 size-3 -translate-y-1/2 text-wb-ink-mute"
				/>
				<Input
					bind:value={filterText}
					placeholder="Filter projects..."
					aria-label="Filter projects"
					class="h-[26px] border-wb-hair bg-wb-bg pr-6 pl-7 font-mono text-[11px] text-wb-ink placeholder:text-wb-ink-mute focus-visible:ring-wb-accent/40"
				/>
				{#if filterText}
					<button
						type="button"
						class="absolute top-[calc(50%+2px)] right-4 -translate-y-1/2 rounded p-0.5 text-wb-ink-mute hover:text-wb-ink"
						aria-label="Clear filter"
						onclick={() => (filterText = '')}
					>
						<XIcon class="size-2.5" />
					</button>
				{/if}
			</div>
		{/if}

		<ScrollArea class="min-h-0 flex-1">
			<div class="pt-1 pb-2">
				{#if !projectStore.loaded}
					<p class="px-3 py-8 text-center font-mono text-[11px] text-wb-ink-mute">Loading...</p>
				{:else if projectStore.projects.length === 0}
					<div class="px-3 py-8 text-center">
						<p class="font-mono text-[11px] text-wb-ink-mute">No projects yet.</p>
						<p class="mt-1 font-mono text-[10.5px] text-wb-ink-mute">
							Add a folder to get started.
						</p>
					</div>
				{:else if filteredGroupedProjects.length === 0}
					<p class="px-3 py-4 text-center font-mono text-[11px] text-wb-ink-mute">No matches.</p>
				{:else}
					{#each filteredGroupedProjects as section (section.group ?? '__ungrouped')}
						{#if section.group}
							{@const isGroupCollapsed = collapsedGroups.has(section.group) && !filterText}
							<button
								class="mt-2 flex w-full items-center gap-1 px-3 py-1 text-left first:mt-0"
								type="button"
								aria-expanded={!isGroupCollapsed}
								onclick={() => toggleSet(collapsedGroups, section.group!)}
							>
								{#if isGroupCollapsed}
									<ChevronRightIcon class="size-3 shrink-0 text-wb-ink-mute" />
								{:else}
									<ChevronDownIcon class="size-3 shrink-0 text-wb-ink-mute" />
								{/if}
								<span
									class="truncate text-[10.5px] font-semibold tracking-wider text-wb-ink-mute uppercase"
									>{section.group}</span
								>
								<span class="ml-auto font-mono text-[10px] text-wb-ink-mute"
									>{section.projects.length}</span
								>
							</button>
							{#if !isGroupCollapsed}
								{#each section.projects as project (project.path)}
									{@render projectRow(project)}
								{/each}
							{/if}
						{:else}
							{#each section.projects as project (project.path)}
								{@render projectRow(project)}
							{/each}
						{/if}
					{/each}
				{/if}
			</div>
		</ScrollArea>
	{/if}
</aside>

{#snippet projectRow(project: ProjectConfig)}
	{@const isActive = workspaceStore.activeProjectPath === project.path}
	{@const sessions = allSessionsForProject(project.path)}
	{@const attentionType = projectAttentionType(project.path)}
	{@const isExpanded = expandedProjects.get(project.path) ?? isActive}
	<div
		role="listitem"
		draggable="true"
		ondragstart={(event) => event.dataTransfer?.setData('text/project-path', project.path)}
		ondragover={(event) => {
			event.preventDefault();
			dragOverProjectPath = project.path;
		}}
		ondragleave={() => {
			if (dragOverProjectPath === project.path) dragOverProjectPath = null;
		}}
		ondrop={(event) => {
			event.preventDefault();
			const fromPath = event.dataTransfer?.getData('text/project-path');
			if (fromPath) projectStore.reorder(fromPath, project.path);
			dragOverProjectPath = null;
		}}
		ondragend={() => {
			dragOverProjectPath = null;
		}}
	>
		<ContextMenu.Root>
			<ContextMenu.Trigger>
				<div
					class={[
						'group flex h-7 items-center gap-1.5 border-l-2 pr-1.5 pl-2 transition-colors',
						dragOverProjectPath === project.path && 'border-t border-t-wb-accent',
						isActive
							? 'border-l-wb-accent bg-wb-panel2 text-wb-ink'
							: 'border-l-transparent text-wb-ink-mute hover:bg-wb-panel2/60 hover:text-wb-ink'
					]}
				>
					<button
						class="flex size-4 shrink-0 items-center justify-center rounded text-wb-ink-mute hover:text-wb-ink"
						type="button"
						aria-label={isExpanded ? `Collapse ${project.name}` : `Expand ${project.name}`}
						aria-expanded={isExpanded}
						onclick={() => toggleExpanded(project.path, !isExpanded, isActive)}
					>
						{#if isExpanded}
							<ChevronDownIcon class="size-3" />
						{:else}
							<ChevronRightIcon class="size-3" />
						{/if}
					</button>
					<button
						class={[
							'min-w-0 flex-1 truncate text-left font-mono text-[12px]',
							isActive && 'font-semibold'
						]}
						type="button"
						onclick={() => projectStore.openProject(project.path)}
					>
						{project.name}
					</button>
					{#if attentionType === 'input'}
						<CircleAlertIcon class="size-3 shrink-0 text-wb-err" />
					{:else if attentionType}
						<CirclePauseIcon
							class={[
								'size-3 shrink-0',
								attentionType === 'codex' ? 'text-wb-codex' : 'text-wb-warn'
							]}
						/>
					{/if}
					{@render sessionBadges(sessions)}
					<DropdownMenu.Root>
						<DropdownMenu.Trigger
							class="grid size-5 shrink-0 place-items-center rounded text-wb-ink-mute opacity-0 transition-opacity group-hover:opacity-100 hover:bg-wb-panel2 hover:text-wb-ink focus-visible:opacity-100 data-[state=open]:opacity-100"
							aria-label="{project.name} actions"
						>
							<EllipsisVerticalIcon class="size-3" />
						</DropdownMenu.Trigger>
						<DropdownMenu.Content align="end" class="w-44">
							<ProjectMenuItems
								{project}
								Item={DropdownMenu.Item}
								Separator={DropdownMenu.Separator}
								Sub={DropdownMenu.Sub}
								SubTrigger={DropdownMenu.SubTrigger}
								SubContent={DropdownMenu.SubContent}
							/>
						</DropdownMenu.Content>
					</DropdownMenu.Root>
				</div>
			</ContextMenu.Trigger>
			<ContextMenu.Content class="w-44">
				<ProjectMenuItems
					{project}
					Item={ContextMenu.Item}
					Separator={ContextMenu.Separator}
					Sub={ContextMenu.Sub}
					SubTrigger={ContextMenu.SubTrigger}
					SubContent={ContextMenu.SubContent}
				/>
			</ContextMenu.Content>
		</ContextMenu.Root>

		{#if isExpanded}
			{@render branchTree(project)}
		{/if}
	</div>
{/snippet}

<!-- main checkout first, then each worktree; sessions nest under the branch they run on -->
{#snippet branchTree(project: ProjectConfig)}
	{@const active = workspaceStore.activeWorkspace}
	{@const mainBranch = gitStore.branchByProject[project.path]}
	{@const mainSessions = allSessionsForProject(project.path).filter((s) => !s.worktreePath)}
	<div class="mb-1 ml-[15px] border-l border-wb-hair">
		<!-- no branch = not a git repo (or git info not loaded yet): no main branch row, no worktree button -->
		{#if mainBranch}
			{@render branchRow({
				project,
				branch: mainBranch,
				isActive: active?.projectPath === project.path && !active.worktreePath,
				sessions: mainSessions,
				onOpen: () => projectStore.openProject(project.path),
				onNewSession: () => claudeSessionStore.startSessionByProject(project.path)
			})}
		{:else}
			{@render sessionList(project, mainSessions)}
		{/if}
		{#each worktreesForProject(project.path) as wt (wt.path)}
			{@render branchRow({
				project,
				branch: wt.branch,
				isActive: active?.worktreePath === wt.path,
				sessions: allSessionsForProject(project.path).filter((s) => s.worktreePath === wt.path),
				onOpen: () => worktreeManager.open(project.path, wt.path, wt.branch),
				onNewSession: () => startSessionInWorktree(project.path, wt.path, wt.branch),
				onRemove: () => worktreeManager.remove(project.path, wt.path, wt.branch)
			})}
		{/each}
		{#if mainBranch}
			<button
				class="flex h-6 w-full items-center gap-1.5 px-2.5 text-left font-mono text-[11px] text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
				type="button"
				onclick={() => worktreeManager.add(project.path)}
			>
				<PlusIcon class="size-3 shrink-0" />
				worktree
			</button>
		{/if}
	</div>
{/snippet}

{#snippet sessionList(project: ProjectConfig, sessions: ActiveClaudeSession[])}
	{#each sessions as session (session.tabId)}
		<div class="pl-5">
			<SessionItem
				{session}
				onSelect={() => workspaceStore.selectTabByProject(project.path, session.tabId)}
				onRestart={() =>
					claudeSessionStore.restartSessionByProject(
						project.path,
						session.tabId,
						session.sessionType
					)}
				onClose={() => workspaceStore.closeTabByProject(project.path, session.tabId)}
			/>
		</div>
	{/each}
{/snippet}

{#snippet branchRow(row: {
	project: ProjectConfig;
	branch: string;
	isActive: boolean;
	sessions: ActiveClaudeSession[];
	onOpen: () => void;
	onNewSession: () => void;
	onRemove?: () => void;
})}
	{@const status = githubStore.getBranchStatus(row.project.path, row.branch)}
	<ContextMenu.Root>
		<ContextMenu.Trigger>
			<div
				class={[
					'group flex h-[26px] items-center gap-1.5 pr-1.5 pl-2.5 font-mono text-[11.5px] transition-colors',
					row.isActive
						? 'bg-wb-accent-soft text-wb-ink'
						: 'text-wb-ink-mute hover:bg-wb-panel2/60 hover:text-wb-ink'
				]}
			>
				<GitBranchIcon
					class={['size-3 shrink-0', row.isActive ? 'text-wb-accent' : 'text-wb-ink-mute']}
				/>
				<button class="min-w-0 flex-1 truncate text-left" type="button" onclick={row.onOpen}>
					{row.branch}
				</button>
				{#if status?.pr}
					<PRStatusBadge pr={status.pr} onClickPr={() => openInGitHub(status.pr!.url)} />
				{:else if status?.branchRuns}
					<CIStatusBadge
						status={status.branchRuns.status}
						onclick={() => githubStore.showBranch(row.project.path, row.branch)}
					/>
				{/if}
				{@render sessionBadges(row.sessions)}
				<button
					class="grid size-5 shrink-0 place-items-center rounded text-wb-ink-mute opacity-0 transition-opacity group-hover:opacity-100 hover:bg-wb-panel2 hover:text-wb-ink focus-visible:opacity-100"
					type="button"
					aria-label="New session on {row.branch}"
					onclick={row.onNewSession}
				>
					<PlusIcon class="size-3" />
				</button>
				{#if row.onRemove}
					<DropdownMenu.Root>
						<DropdownMenu.Trigger
							class="grid size-5 shrink-0 place-items-center rounded text-wb-ink-mute opacity-0 transition-opacity group-hover:opacity-100 hover:bg-wb-panel2 hover:text-wb-ink focus-visible:opacity-100 data-[state=open]:opacity-100"
							aria-label="{row.branch} actions"
						>
							<EllipsisVerticalIcon class="size-3" />
						</DropdownMenu.Trigger>
						<DropdownMenu.Content align="end" class="w-44">
							{@render branchMenuItems(row, DropdownMenu.Item, DropdownMenu.Separator)}
						</DropdownMenu.Content>
					</DropdownMenu.Root>
				{/if}
			</div>
		</ContextMenu.Trigger>
		<ContextMenu.Content class="w-44">
			{@render branchMenuItems(row, ContextMenu.Item, ContextMenu.Separator)}
		</ContextMenu.Content>
	</ContextMenu.Root>
	{@render sessionList(row.project, row.sessions)}
{/snippet}

{#snippet branchMenuItems(
	row: { onOpen: () => void; onNewSession: () => void; onRemove?: () => void },
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	Item: Component<any>,
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	Separator: Component<any>
)}
	<Item onclick={row.onOpen}>
		<ExternalLinkIcon class="size-3.5" />
		Open
	</Item>
	<Item onclick={row.onNewSession}>
		<SparklesIcon class="size-3.5" />
		New Session
	</Item>
	{#if row.onRemove}
		<Separator />
		<Item class="text-destructive" onclick={row.onRemove}>
			<Trash2Icon class="size-3.5" />
			Remove Worktree
		</Item>
	{/if}
{/snippet}

{#snippet sessionBadges(sessions: ActiveClaudeSession[])}
	{@const claude = sessionCount(sessions, 'claude')}
	{@const codex = sessionCount(sessions, 'codex')}
	{#if claude > 0}
		<span
			class="shrink-0 rounded bg-wb-claude/20 px-1 font-mono text-[9.5px] font-semibold text-wb-claude"
			>C{claude}</span
		>
	{/if}
	{#if codex > 0}
		<span
			class="shrink-0 rounded bg-wb-codex/20 px-1 font-mono text-[9.5px] font-semibold text-wb-codex"
			>X{codex}</span
		>
	{/if}
{/snippet}

<CloneRepoDialog bind:open={cloneDialogOpen} />
