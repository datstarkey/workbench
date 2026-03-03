<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import CirclePauseIcon from '@lucide/svelte/icons/circle-pause';
	import EllipsisVerticalIcon from '@lucide/svelte/icons/ellipsis-vertical';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import PanelLeftCloseIcon from '@lucide/svelte/icons/panel-left-close';
	import PanelLeftOpenIcon from '@lucide/svelte/icons/panel-left-open';
	import PlayIcon from '@lucide/svelte/icons/play';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import SearchIcon from '@lucide/svelte/icons/search';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import TerminalSquareIcon from '@lucide/svelte/icons/terminal-square';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Input } from '$lib/components/ui/input';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { SvelteSet } from 'svelte/reactivity';
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
	import type {
		ActiveClaudeSession,
		ProjectConfig,
		ProjectTask,
		WorktreeInfo
	} from '$types/workbench';
	import type { ProjectGroup } from '$stores/projects.svelte';
	import CIStatusBadge from './CIStatusBadge.svelte';
	import PRStatusBadge from './PRStatusBadge.svelte';
	import ProjectMenuItems from './ProjectMenuItems.svelte';
	import SessionItem from './SessionItem.svelte';

	const projectStore = getProjectStore();
	const workspaceStore = getWorkspaceStore();
	const claudeSessionStore = getClaudeSessionStore();
	const gitStore = getGitStore();
	const githubStore = getGitHubStore();
	const projectManager = getProjectManager();
	const worktreeManager = getWorktreeManager();

	let {
		sidebarCollapsed,
		onOpenSettings,
		onToggleSidebar
	}: {
		sidebarCollapsed: boolean;
		onOpenSettings: () => void;
		onToggleSidebar: () => void;
	} = $props();

	const expandedProjects = new SvelteSet<string>();
	const expandedWorktrees = new SvelteSet<string>();
	const collapsedGroups = new SvelteSet<string>();

	let filterText = $state('');

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

	function allSessionsForProject(projectPath: string): ActiveClaudeSession[] {
		return claudeSessionStore.activeSessionsByProject[projectPath] ?? [];
	}

	function mainSessionsForProject(projectPath: string): ActiveClaudeSession[] {
		return allSessionsForProject(projectPath).filter((s) => !s.worktreePath);
	}

	function sessionsForWorktree(projectPath: string, worktreePath: string): ActiveClaudeSession[] {
		return allSessionsForProject(projectPath).filter((s) => s.worktreePath === worktreePath);
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

	function tasksForProject(project: ProjectConfig): ProjectTask[] {
		return project.tasks ?? [];
	}

	function runTask(project: ProjectConfig, task: ProjectTask): void {
		projectStore.openProject(project.path);
		workspaceStore.runTaskByProject(project.path, task);
	}

	function runTaskInWorktree(
		project: ProjectConfig,
		worktreePath: string,
		branch: string,
		task: ProjectTask
	): void {
		worktreeManager.open(project.path, worktreePath, branch);
		const ws = workspaceStore.getByWorktreePath(worktreePath);
		if (ws) {
			workspaceStore.runTaskInWorkspace(ws.id, task);
		}
	}

	function startSessionInWorktree(projectPath: string, worktreePath: string, branch: string): void {
		worktreeManager.open(projectPath, worktreePath, branch);
		const ws = workspaceStore.getByWorktreePath(worktreePath);
		if (ws) {
			claudeSessionStore.startSessionInWorkspace(ws);
		}
	}

	function worktreeHasChildren(
		projectPath: string,
		worktreePath: string,
		tasks: ProjectTask[]
	): boolean {
		return sessionsForWorktree(projectPath, worktreePath).length > 0 || tasks.length > 0;
	}

	function hasExpandableContent(projectPath: string, tasks: ProjectTask[]): boolean {
		return (
			allSessionsForProject(projectPath).length > 0 ||
			worktreesForProject(projectPath).length > 0 ||
			tasks.length > 0
		);
	}

	let dragOverProjectPath = $state<string | null>(null);
</script>

<aside class="flex h-full w-full flex-col overflow-hidden bg-muted/20">
	<div class="flex h-10 shrink-0 items-center border-b border-border/60 px-2">
		{#if !sidebarCollapsed}
			<div class="flex items-center gap-2 overflow-hidden">
				<div
					class="flex size-6 shrink-0 items-center justify-center rounded bg-primary/15 text-primary"
				>
					<TerminalSquareIcon class="size-3.5" />
				</div>
				<span class="truncate text-sm font-semibold tracking-tight">Workbench</span>
			</div>
		{/if}
		<Button
			variant="ghost"
			size="icon-sm"
			class={`shrink-0 text-muted-foreground hover:text-foreground ${sidebarCollapsed ? 'mx-auto' : 'ml-auto'}`}
			type="button"
			aria-label="Toggle sidebar"
			onclick={onToggleSidebar}
		>
			{#if sidebarCollapsed}
				<PanelLeftOpenIcon class="size-3.5" />
			{:else}
				<PanelLeftCloseIcon class="size-3.5" />
			{/if}
		</Button>
	</div>

	{#if !sidebarCollapsed}
		<div class="shrink-0 space-y-2 p-2">
			<Button
				type="button"
				variant="outline"
				size="sm"
				class="w-full justify-start gap-2 text-muted-foreground hover:text-foreground"
				onclick={() => projectManager.add()}
			>
				<PlusIcon class="size-3.5" />
				Add Project
			</Button>
			{#if projectStore.projects.length > 0}
				<div class="relative">
					<SearchIcon
						class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground/60"
					/>
					<Input
						bind:value={filterText}
						placeholder="Filter projects..."
						class="h-7 pr-7 pl-7 text-xs"
					/>
					{#if filterText}
						<button
							type="button"
							class="absolute top-1/2 right-1.5 -translate-y-1/2 rounded p-0.5 text-muted-foreground/60 hover:text-foreground"
							onclick={() => (filterText = '')}
						>
							<XIcon class="size-3" />
						</button>
					{/if}
				</div>
			{/if}
		</div>

		<ScrollArea class="min-h-0 flex-1">
			<div class="space-y-0.5 px-2 pb-2">
				{#if !projectStore.loaded}
					<p class="px-2 py-8 text-center text-xs text-muted-foreground">Loading...</p>
				{:else if projectStore.projects.length === 0}
					<div class="px-2 py-8 text-center">
						<p class="text-xs text-muted-foreground">No projects yet.</p>
						<p class="mt-1 text-xs text-muted-foreground/60">Add a folder to get started.</p>
					</div>
				{:else if filteredGroupedProjects.length === 0}
					<p class="px-2 py-4 text-center text-xs text-muted-foreground">No matches.</p>
				{:else}
					{#each filteredGroupedProjects as section (section.group ?? '__ungrouped')}
						{#if section.group}
							{@const isGroupCollapsed = collapsedGroups.has(section.group) && !filterText}
							<button
								class="mt-2 flex w-full items-center gap-1 rounded px-1.5 py-1 text-left first:mt-0"
								type="button"
								onclick={() => toggleSet(collapsedGroups, section.group!)}
							>
								{#if isGroupCollapsed}
									<ChevronRightIcon class="size-3 shrink-0 text-muted-foreground/60" />
								{:else}
									<ChevronDownIcon class="size-3 shrink-0 text-muted-foreground/60" />
								{/if}
								<span
									class="truncate text-[11px] font-semibold tracking-wider text-muted-foreground/60 uppercase"
									>{section.group}</span
								>
								<span class="ml-auto text-[10px] text-muted-foreground/40"
									>{section.projects.length}</span
								>
							</button>
							{#if isGroupCollapsed}
								<!-- Group collapsed — skip rendering projects -->
							{:else}
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

		<div class="shrink-0 border-t border-border/60 p-2">
			<Button
				type="button"
				variant="ghost"
				size="sm"
				class="w-full justify-start gap-2 text-muted-foreground hover:text-foreground"
				onclick={onOpenSettings}
			>
				<SettingsIcon class="size-3.5" />
				Settings
			</Button>
		</div>
	{:else}
		<div class="flex flex-1 flex-col items-center gap-1 pt-2">
			<Tooltip.Root>
				<Tooltip.Trigger>
					<Button
						variant="ghost"
						size="icon-sm"
						class="text-muted-foreground hover:text-foreground"
						type="button"
						onclick={() => projectManager.add()}
					>
						<PlusIcon class="size-3.5" />
					</Button>
				</Tooltip.Trigger>
				<Tooltip.Content side="right">Add Project</Tooltip.Content>
			</Tooltip.Root>
		</div>

		<div class="shrink-0 border-t border-border/60 p-1">
			<Tooltip.Root>
				<Tooltip.Trigger>
					<Button
						variant="ghost"
						size="icon-sm"
						class="mx-auto text-muted-foreground hover:text-foreground"
						type="button"
						onclick={onOpenSettings}
					>
						<SettingsIcon class="size-3.5" />
					</Button>
				</Tooltip.Trigger>
				<Tooltip.Content side="right">Settings</Tooltip.Content>
			</Tooltip.Root>
		</div>
	{/if}
</aside>

{#snippet projectRow(project: ProjectConfig)}
	{@const isOpen = workspaceStore.openProjectPaths.includes(project.path)}
	{@const isActive = workspaceStore.activeProjectPath === project.path}
	{@const mainSessions = mainSessionsForProject(project.path)}
	{@const worktrees = worktreesForProject(project.path)}
	{@const tasks = tasksForProject(project)}
	{@const branch = gitStore.branchByProject[project.path]}
	{@const attentionType = projectAttentionType(project.path)}
	{@const hasAttention = attentionType !== null}
	{@const hasChildren = hasExpandableContent(project.path, tasks)}
	{@const isExpanded =
		expandedProjects.has(project.path) || project.path === workspaceStore.activeProjectPath}
	{@const isDragOver = dragOverProjectPath === project.path}
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
					class={`group flex items-center gap-1 rounded-md px-1.5 py-1.5 transition-colors ${isDragOver ? 'border-t-2 border-primary' : 'border-t-2 border-transparent'} ${isActive ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground'}`}
				>
					{#if hasChildren}
						<button
							class="flex size-4 shrink-0 items-center justify-center rounded hover:bg-muted"
							type="button"
							aria-label={isExpanded ? 'Collapse' : 'Expand'}
							aria-expanded={isExpanded}
							onclick={() => toggleSet(expandedProjects, project.path)}
						>
							{#if isExpanded}
								<ChevronDownIcon class="size-3" />
							{:else}
								<ChevronRightIcon class="size-3" />
							{/if}
						</button>
					{:else}
						<div class="size-4 shrink-0"></div>
					{/if}
					<button
						class="flex min-w-0 flex-1 items-center gap-2 text-left"
						type="button"
						onclick={() => projectStore.openProject(project.path)}
					>
						<FolderIcon class="size-3.5 shrink-0 opacity-60" />
						<span class="truncate text-sm">{project.name}</span>
						{#if branch}
							<span class="truncate text-[10px] text-muted-foreground/60">({branch})</span>
							{@const branchStatus = githubStore.getBranchStatus(project.path, branch)}
							{#if branchStatus?.pr}
								<PRStatusBadge
									pr={branchStatus.pr}
									onClickPr={() => openInGitHub(branchStatus.pr!.url)}
								/>
							{:else if branchStatus?.branchRuns}
								<CIStatusBadge
									status={branchStatus.branchRuns.status}
									onclick={() => githubStore.showBranch(project.path, branch)}
								/>
							{/if}
						{/if}
						{#if attentionType === 'input'}
							<CircleAlertIcon class="size-3 shrink-0 text-red-400" />
						{:else if hasAttention}
							<CirclePauseIcon
								class={`size-3 shrink-0 ${attentionType === 'codex' ? 'text-sky-400' : 'text-amber-400'}`}
							/>
						{:else if isOpen}
							<span class="size-1.5 shrink-0 rounded-full bg-primary"></span>
						{/if}
					</button>
					<DropdownMenu.Root>
						<DropdownMenu.Trigger>
							<Button
								variant="ghost"
								size="icon-sm"
								class="size-6 shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
							>
								<EllipsisVerticalIcon class="size-3.5" />
							</Button>
						</DropdownMenu.Trigger>
						<DropdownMenu.Content align="end" class="w-44">
							<ProjectMenuItems
								{project}
								{tasks}
								Item={DropdownMenu.Item}
								Separator={DropdownMenu.Separator}
								Group={DropdownMenu.Group}
								GroupHeading={DropdownMenu.GroupHeading}
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
					{tasks}
					Item={ContextMenu.Item}
					Separator={ContextMenu.Separator}
					Group={ContextMenu.Group}
					GroupHeading={ContextMenu.GroupHeading}
					Sub={ContextMenu.Sub}
					SubTrigger={ContextMenu.SubTrigger}
					SubContent={ContextMenu.SubContent}
				/>
			</ContextMenu.Content>
		</ContextMenu.Root>

		{#if isExpanded && hasChildren}
			<div class="mt-0.5 ml-5 space-y-0.5 border-l border-border/40 pl-2">
				{#if tasks.length > 0}
					{#each tasks as task, i (`${task.name}-${i}`)}
						<button
							class="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-muted-foreground transition-colors hover:bg-accent/50 hover:text-foreground"
							type="button"
							onclick={() => runTask(project, task)}
						>
							<PlayIcon class="size-3 shrink-0 text-cyan-400" />
							<span class="truncate text-xs font-medium">{task.name}</span>
						</button>
					{/each}
				{/if}
				{#if worktrees.length > 0}
					{#each worktrees as wt (wt.path)}
						{@const wtSessions = sessionsForWorktree(project.path, wt.path)}
						{@const wtHasChildren = worktreeHasChildren(project.path, wt.path, tasks)}
						{@const wtExpanded = expandedWorktrees.has(wt.path)}
						{@const wtBranchStatus = githubStore.getBranchStatus(project.path, wt.branch)}
						<div>
							<ContextMenu.Root>
								<ContextMenu.Trigger class="w-full">
									<button
										class="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-muted-foreground transition-colors hover:bg-accent/50 hover:text-foreground"
										type="button"
										onclick={() => {
											if (wtHasChildren) {
												toggleSet(expandedWorktrees, wt.path);
											} else {
												worktreeManager.open(project.path, wt.path, wt.branch);
											}
										}}
									>
										{#if wtHasChildren}
											{#if wtExpanded}
												<ChevronDownIcon class="size-3 shrink-0 text-emerald-400" />
											{:else}
												<ChevronRightIcon class="size-3 shrink-0 text-emerald-400" />
											{/if}
										{:else}
											<GitBranchIcon class="size-3 shrink-0 text-emerald-400" />
										{/if}
										<span class="truncate text-xs font-medium">{wt.branch}</span>
										{#if wtBranchStatus?.pr}
											<PRStatusBadge
												pr={wtBranchStatus.pr}
												onClickPr={() => openInGitHub(wtBranchStatus.pr!.url)}
											/>
										{:else if wtBranchStatus?.branchRuns}
											<CIStatusBadge
												status={wtBranchStatus.branchRuns.status}
												onclick={() => githubStore.showBranch(project.path, wt.branch)}
											/>
										{/if}
									</button>
								</ContextMenu.Trigger>
								<ContextMenu.Content class="w-44">
									<ContextMenu.Item
										onclick={() => worktreeManager.open(project.path, wt.path, wt.branch)}
									>
										<ExternalLinkIcon class="size-3.5" />
										Open
									</ContextMenu.Item>
									<ContextMenu.Item
										onclick={() => startSessionInWorktree(project.path, wt.path, wt.branch)}
									>
										<SparklesIcon class="size-3.5" />
										New Session
									</ContextMenu.Item>
									<ContextMenu.Separator />
									<ContextMenu.Item
										class="text-destructive"
										onclick={() => worktreeManager.remove(project.path, wt.path, wt.branch)}
									>
										<Trash2Icon class="size-3.5" />
										Remove
									</ContextMenu.Item>
								</ContextMenu.Content>
							</ContextMenu.Root>
							{#if wtExpanded && wtHasChildren}
								<div class="mt-0.5 ml-4 space-y-0.5 border-l border-border/30 pl-2">
									{#if tasks.length > 0}
										{#each tasks as task, i (`wt-${wt.path}-${task.name}-${i}`)}
											<button
												class="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-muted-foreground transition-colors hover:bg-accent/50 hover:text-foreground"
												type="button"
												onclick={() => runTaskInWorktree(project, wt.path, wt.branch, task)}
											>
												<PlayIcon class="size-3 shrink-0 text-cyan-400" />
												<span class="truncate text-xs font-medium">{task.name}</span>
											</button>
										{/each}
									{/if}
									{#each wtSessions as session (session.tabId)}
										<SessionItem
											{session}
											onSelect={() =>
												workspaceStore.selectTabByProject(project.path, session.tabId)}
											onRestart={() =>
												claudeSessionStore.restartSessionByProject(
													project.path,
													session.tabId,
													session.sessionType
												)}
											onClose={() => workspaceStore.closeTabByProject(project.path, session.tabId)}
										/>
									{/each}
									<button
										class="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-muted-foreground/60 transition-colors hover:bg-accent/50 hover:text-foreground"
										type="button"
										onclick={() => startSessionInWorktree(project.path, wt.path, wt.branch)}
									>
										<PlusIcon class="size-3 shrink-0" />
										<span class="text-xs">New Session</span>
									</button>
								</div>
							{/if}
						</div>
					{/each}
					<button
						class="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-muted-foreground/60 transition-colors hover:bg-accent/50 hover:text-foreground"
						type="button"
						onclick={() => worktreeManager.add(project.path)}
					>
						<PlusIcon class="size-3 shrink-0" />
						<span class="text-xs">Add Worktree</span>
					</button>
				{/if}
				{#each mainSessions as session (session.tabId)}
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
				{/each}
				{#if mainSessions.length > 0}
					<button
						class="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-muted-foreground/60 transition-colors hover:bg-accent/50 hover:text-foreground"
						type="button"
						onclick={() => claudeSessionStore.startSessionByProject(project.path)}
					>
						<PlusIcon class="size-3 shrink-0" />
						<span class="text-xs">New Session</span>
					</button>
				{/if}
			</div>
		{/if}
	</div>
{/snippet}
