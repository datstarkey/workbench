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
	import DownloadIcon from '@lucide/svelte/icons/download';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import InstanceSwitcher from '$features/instances/InstanceSwitcher.svelte';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '@workbench/ui/button';
	import * as ContextMenu from '@workbench/ui/context-menu';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import { Input } from '@workbench/ui/input';
	import { ScrollArea } from '@workbench/ui/scroll-area';
	import * as Tooltip from '@workbench/ui/tooltip';
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
		onOpenSettings,
		onToggleSidebar,
		onConnect
	}: {
		sidebarCollapsed: boolean;
		onOpenSettings: () => void;
		onToggleSidebar: () => void;
		onConnect: () => void;
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
	let cloneDialogOpen = $state(false);

	/** Returns parent dir portion for the project path hint */
	function parentDir(path: string): string {
		const parts = path.split('/');
		return parts.length >= 2 ? parts[parts.length - 2] : '';
	}

	function claudeCount(sessions: ActiveClaudeSession[]): number {
		return sessions.filter((s) => s.sessionType === 'claude').length;
	}

	function codexCount(sessions: ActiveClaudeSession[]): number {
		return sessions.filter((s) => s.sessionType === 'codex').length;
	}
</script>

<aside class="flex h-full w-full flex-col overflow-hidden border-r border-wb-hair bg-wb-panel">
	<!-- Header row -->
	<div class="flex h-[38px] shrink-0 items-center border-b border-wb-hair px-2.5">
		{#if !sidebarCollapsed}
			<InstanceSwitcher {onConnect} />
			<div class="ml-auto flex items-center gap-0.5">
				<Tooltip.Root>
					<Tooltip.Trigger>
						<button
							class="flex size-[22px] items-center justify-center rounded text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
							type="button"
							onclick={() => projectManager.add()}
							aria-label="Add project"
						>
							<PlusIcon class="size-3" />
						</button>
					</Tooltip.Trigger>
					<Tooltip.Content side="bottom">Add Project</Tooltip.Content>
				</Tooltip.Root>
				<Tooltip.Root>
					<Tooltip.Trigger>
						<button
							class="flex size-[22px] items-center justify-center rounded text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
							type="button"
							onclick={() => (cloneDialogOpen = true)}
							aria-label="Clone from GitHub"
						>
							<DownloadIcon class="size-3" />
						</button>
					</Tooltip.Trigger>
					<Tooltip.Content side="bottom">Clone from GitHub</Tooltip.Content>
				</Tooltip.Root>
				<button
					class="flex size-[22px] items-center justify-center rounded text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
					type="button"
					aria-label="Toggle sidebar"
					onclick={onToggleSidebar}
				>
					<PanelLeftCloseIcon class="size-3" />
				</button>
			</div>
		{:else}
			<button
				class="mx-auto flex size-[22px] items-center justify-center rounded text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
				type="button"
				aria-label="Toggle sidebar"
				onclick={onToggleSidebar}
			>
				<PanelLeftOpenIcon class="size-3" />
			</button>
		{/if}
	</div>

	{#if !sidebarCollapsed}
		<!-- Filter input -->
		{#if projectStore.projects.length > 0}
			<div class="relative shrink-0 px-2 py-1.5">
				<SearchIcon
					class="pointer-events-none absolute top-1/2 left-3.5 size-3 -translate-y-1/2 text-wb-ink-soft"
				/>
				<Input
					bind:value={filterText}
					placeholder="Filter projects..."
					class="h-6 border-wb-hair bg-wb-bg pr-6 pl-6 font-mono text-[11px] text-wb-ink-mute placeholder:text-wb-ink-soft focus-visible:ring-wb-accent/40"
				/>
				{#if filterText}
					<button
						type="button"
						class="absolute top-1/2 right-3.5 -translate-y-1/2 rounded p-0.5 text-wb-ink-soft hover:text-wb-ink"
						onclick={() => (filterText = '')}
					>
						<XIcon class="size-2.5" />
					</button>
				{/if}
			</div>
		{/if}

		<ScrollArea class="min-h-0 flex-1">
			<div class="space-y-0 pb-2">
				{#if !projectStore.loaded}
					<p class="px-3 py-8 text-center font-mono text-[11px] text-wb-ink-soft">Loading...</p>
				{:else if projectStore.projects.length === 0}
					<div class="px-3 py-8 text-center">
						<p class="font-mono text-[11px] text-wb-ink-mute">No projects yet.</p>
						<p class="mt-1 font-mono text-[10.5px] text-wb-ink-soft">
							Add a folder to get started.
						</p>
					</div>
				{:else if filteredGroupedProjects.length === 0}
					<p class="px-3 py-4 text-center font-mono text-[11px] text-wb-ink-soft">No matches.</p>
				{:else}
					{#each filteredGroupedProjects as section (section.group ?? '__ungrouped')}
						{#if section.group}
							{@const isGroupCollapsed = collapsedGroups.has(section.group) && !filterText}
							<button
								class="mt-2 flex w-full items-center gap-1 px-3 py-1 text-left first:mt-0"
								type="button"
								onclick={() => toggleSet(collapsedGroups, section.group!)}
							>
								{#if isGroupCollapsed}
									<ChevronRightIcon class="size-3 shrink-0 text-wb-ink-soft" />
								{:else}
									<ChevronDownIcon class="size-3 shrink-0 text-wb-ink-soft" />
								{/if}
								<span
									class="truncate text-[10.5px] font-semibold tracking-wider text-wb-ink-soft uppercase"
									>{section.group}</span
								>
								<span class="ml-auto font-mono text-[10px] text-wb-ink-soft"
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

		<div class="shrink-0 border-t border-wb-hair p-1.5">
			<button
				type="button"
				class="flex w-full items-center gap-2 rounded px-2 py-1.5 text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
				onclick={onOpenSettings}
			>
				<SettingsIcon class="size-3 shrink-0" />
				<span class="font-mono text-[11px]">Settings</span>
			</button>
		</div>
	{:else}
		<div class="flex flex-1 flex-col items-center gap-1 pt-2">
			<Tooltip.Root>
				<Tooltip.Trigger>
					<button
						class="flex size-[22px] items-center justify-center rounded text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
						type="button"
						onclick={() => projectManager.add()}
					>
						<PlusIcon class="size-3" />
					</button>
				</Tooltip.Trigger>
				<Tooltip.Content side="right">Add Project</Tooltip.Content>
			</Tooltip.Root>
		</div>

		<div class="shrink-0 border-t border-wb-hair p-1">
			<Tooltip.Root>
				<Tooltip.Trigger>
					<button
						class="mx-auto flex size-[22px] items-center justify-center rounded text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
						type="button"
						onclick={onOpenSettings}
					>
						<SettingsIcon class="size-3" />
					</button>
				</Tooltip.Trigger>
				<Tooltip.Content side="right">Settings</Tooltip.Content>
			</Tooltip.Root>
		</div>
	{/if}
</aside>

{#snippet projectRow(project: ProjectConfig)}
	{@const isOpen = workspaceStore.isProjectOpen(project.path)}
	{@const isActive = workspaceStore.activeProjectPath === project.path}
	{@const mainSessions = mainSessionsForProject(project.path)}
	{@const worktrees = worktreesForProject(project.path)}
	{@const tasks = project.tasks ?? []}
	{@const branch = gitStore.branchByProject[project.path]}
	{@const attentionType = projectAttentionType(project.path)}
	{@const hasAttention = attentionType !== null}
	{@const hasChildren = hasExpandableContent(project.path, tasks)}
	{@const isExpanded =
		expandedProjects.has(project.path) || project.path === workspaceStore.activeProjectPath}
	{@const isDragOver = dragOverProjectPath === project.path}
	{@const cCount = claudeCount(mainSessions)}
	{@const xCount = codexCount(mainSessions)}
	{@const hint = parentDir(project.path)}
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
						'group flex items-center gap-1 border-l-2 px-2.5 py-[5px] transition-colors',
						isDragOver ? 'border-t border-wb-accent' : '',
						isActive
							? 'border-l-wb-accent bg-wb-panel2 text-wb-ink'
							: 'border-l-transparent text-wb-ink-mute hover:bg-wb-panel2/60 hover:text-wb-ink'
					]}
				>
					{#if hasChildren}
						<button
							class="flex size-4 shrink-0 items-center justify-center rounded text-wb-ink-soft hover:text-wb-ink"
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
						class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
						type="button"
						onclick={() => projectStore.openProject(project.path)}
					>
						<FolderIcon
							class={['size-3.5 shrink-0', isActive ? 'text-wb-accent' : 'text-wb-ink-soft']}
						/>
						<span class="truncate font-mono text-[12px]">{project.name}</span>
						{#if branch}
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
					</button>
					<!-- Right-side meta: session badges + attention + hint -->
					<div class="flex shrink-0 items-center gap-1">
						{#if attentionType === 'input'}
							<CircleAlertIcon class="size-3 shrink-0 text-wb-err" />
						{:else if hasAttention}
							<CirclePauseIcon
								class={[
									'size-3 shrink-0',
									attentionType === 'codex' ? 'text-wb-codex' : 'text-wb-warn'
								]}
							/>
						{:else if isOpen}
							<span class="size-1.5 shrink-0 rounded-full bg-wb-accent"></span>
						{/if}
						{@render sessionBadges(cCount, xCount)}
						{#if hint}
							<span
								class="hidden max-w-[60px] truncate font-mono text-[10.5px] text-wb-ink-soft group-hover:block"
								>{hint}</span
							>
						{/if}
					</div>
					<DropdownMenu.Root>
						<DropdownMenu.Trigger>
							<Button
								variant="ghost"
								size="icon-sm"
								class="size-5 shrink-0 text-wb-ink-soft opacity-0 transition-opacity group-hover:opacity-100 hover:bg-wb-panel2 hover:text-wb-ink"
							>
								<EllipsisVerticalIcon class="size-3" />
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
			<div class="mt-0 ml-7 space-y-0 border-l border-wb-hair-soft pl-2">
				{#if tasks.length > 0}
					{#each tasks as task, i (`${task.name}-${i}`)}
						<button
							class="flex w-full items-center gap-2 px-2 py-1 text-left font-mono text-[11px] text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
							type="button"
							onclick={() => runTask(project, task)}
						>
							<PlayIcon class="size-3 shrink-0 text-wb-ok" />
							<span class="truncate">{task.name}</span>
						</button>
					{/each}
				{/if}
				{#if worktrees.length > 0}
					{#each worktrees as wt (wt.path)}
						{@const wtSessions = sessionsForWorktree(project.path, wt.path)}
						{@const wtHasChildren = worktreeHasChildren(project.path, wt.path, tasks)}
						{@const wtExpanded = expandedWorktrees.has(wt.path)}
						{@const wtBranchStatus = githubStore.getBranchStatus(project.path, wt.branch)}
						{@const wtActive = workspaceStore.getByWorktreePath(wt.path) !== null}
						{@const wtCCount = claudeCount(wtSessions)}
						{@const wtXCount = codexCount(wtSessions)}
						<div>
							<ContextMenu.Root>
								<ContextMenu.Trigger class="w-full">
									<button
										class={[
											'flex w-full items-center gap-1.5 border-l-2 py-[4px] pr-2 pl-1 text-left font-mono text-[11.5px] transition-colors',
											wtActive
												? 'border-l-wb-accent bg-wb-accent-soft text-wb-ink'
												: 'border-l-transparent text-wb-ink-mute hover:bg-wb-panel2/60 hover:text-wb-ink'
										]}
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
												<ChevronDownIcon class="size-3 shrink-0 text-wb-ink-soft" />
											{:else}
												<ChevronRightIcon class="size-3 shrink-0 text-wb-ink-soft" />
											{/if}
										{:else}
											<GitBranchIcon class="size-3 shrink-0 text-wb-ink-soft" />
										{/if}
										<span class="truncate">{wt.branch}</span>
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
										<div class="ml-auto flex shrink-0 items-center gap-1">
											{@render sessionBadges(wtCCount, wtXCount)}
										</div>
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
								<div class="mt-0 ml-4 space-y-0 border-l border-wb-hair-soft pl-2">
									{#if tasks.length > 0}
										{#each tasks as task, i (`wt-${wt.path}-${task.name}-${i}`)}
											<button
												class="flex w-full items-center gap-2 px-2 py-1 text-left font-mono text-[11px] text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
												type="button"
												onclick={() => runTaskInWorktree(project, wt.path, wt.branch, task)}
											>
												<PlayIcon class="size-3 shrink-0 text-wb-ok" />
												<span class="truncate">{task.name}</span>
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
										class="flex w-full items-center gap-2 px-2 py-1 text-left font-mono text-[11px] text-wb-ink-soft transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
										type="button"
										onclick={() => startSessionInWorktree(project.path, wt.path, wt.branch)}
									>
										<PlusIcon class="size-3 shrink-0" />
										<span>New Session</span>
									</button>
								</div>
							{/if}
						</div>
					{/each}
					<button
						class="flex w-full items-center gap-2 px-2 py-1 text-left font-mono text-[11px] text-wb-ink-soft transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
						type="button"
						onclick={() => worktreeManager.add(project.path)}
					>
						<PlusIcon class="size-3 shrink-0" />
						<span>Add Worktree</span>
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
						class="flex w-full items-center gap-2 px-2 py-1 text-left font-mono text-[11px] text-wb-ink-soft transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
						type="button"
						onclick={() => claudeSessionStore.startSessionByProject(project.path)}
					>
						<PlusIcon class="size-3 shrink-0" />
						<span>New Session</span>
					</button>
				{/if}
			</div>
		{/if}
	</div>
{/snippet}

{#snippet sessionBadges(cCount: number, xCount: number)}
	{#if cCount > 0}
		<span class="rounded bg-wb-claude/20 px-1 font-mono text-[9.5px] font-semibold text-wb-claude"
			>C{cCount}</span
		>
	{/if}
	{#if xCount > 0}
		<span class="rounded bg-wb-codex/20 px-1 font-mono text-[9.5px] font-semibold text-wb-codex"
			>X{xCount}</span
		>
	{/if}
{/snippet}

<CloneRepoDialog bind:open={cloneDialogOpen} />
