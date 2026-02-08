<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import CodeIcon from '@lucide/svelte/icons/code';
	import EllipsisVerticalIcon from '@lucide/svelte/icons/ellipsis-vertical';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import PanelLeftCloseIcon from '@lucide/svelte/icons/panel-left-close';
	import PanelLeftOpenIcon from '@lucide/svelte/icons/panel-left-open';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import PlayIcon from '@lucide/svelte/icons/play';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import TerminalSquareIcon from '@lucide/svelte/icons/terminal-square';
	import CirclePauseIcon from '@lucide/svelte/icons/circle-pause';
	import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { SvelteSet } from 'svelte/reactivity';
	import {
		getClaudeSessionStore,
		getGitStore,
		getProjectManager,
		getProjectStore,
		getWorktreeManager,
		getWorkspaceStore
	} from '$stores/context';
	import { openInVSCode } from '$lib/utils/vscode';
	import type {
		ActiveClaudeSession,
		ProjectConfig,
		ProjectTask,
		WorktreeInfo
	} from '$types/workbench';

	const projectStore = getProjectStore();
	const workspaceStore = getWorkspaceStore();
	const claudeSessionStore = getClaudeSessionStore();
	const gitStore = getGitStore();
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

	function toggleExpanded(path: string) {
		if (expandedProjects.has(path)) {
			expandedProjects.delete(path);
		} else {
			expandedProjects.add(path);
		}
	}

	function sessionsForProject(projectPath: string): ActiveClaudeSession[] {
		return claudeSessionStore.activeSessionsByProject[projectPath] ?? [];
	}

	function worktreesForProject(projectPath: string): WorktreeInfo[] {
		return (gitStore.worktreesByProject[projectPath] ?? []).filter((wt) => !wt.isMain);
	}

	function projectHasAttention(projectPath: string): boolean {
		return sessionsForProject(projectPath).some((s) => s.needsAttention);
	}

	function tasksForProject(project: ProjectConfig): ProjectTask[] {
		return project.tasks ?? [];
	}

	function runTask(project: ProjectConfig, task: ProjectTask): void {
		projectStore.openProject(project.path);
		workspaceStore.runTaskByProject(project.path, task);
	}

	function hasExpandableContent(projectPath: string): boolean {
		return (
			sessionsForProject(projectPath).length > 0 ||
			worktreesForProject(projectPath).length > 0 ||
			(projectStore.getByPath(projectPath)?.tasks?.length ?? 0) > 0
		);
	}
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
		<div class="shrink-0 p-2">
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
		</div>

		<ScrollArea class="flex-1">
			<div class="space-y-0.5 px-2 pb-2">
				{#if !projectStore.loaded}
					<p class="px-2 py-8 text-center text-xs text-muted-foreground">Loading...</p>
				{:else if projectStore.projects.length === 0}
					<div class="px-2 py-8 text-center">
						<p class="text-xs text-muted-foreground">No projects yet.</p>
						<p class="mt-1 text-xs text-muted-foreground/60">Add a folder to get started.</p>
					</div>
				{:else}
					{#each projectStore.projects as project (project.path)}
						{@const isOpen = workspaceStore.openProjectPaths.includes(project.path)}
						{@const isActive = workspaceStore.activeProjectPath === project.path}
						{@const sessions = sessionsForProject(project.path)}
						{@const worktrees = worktreesForProject(project.path)}
						{@const tasks = tasksForProject(project)}
						{@const branch = gitStore.branchByProject[project.path]}
						{@const hasAttention = projectHasAttention(project.path)}
						{@const hasChildren = hasExpandableContent(project.path)}
						{@const isExpanded =
							expandedProjects.has(project.path) ||
							project.path === workspaceStore.activeProjectPath}
						<div>
							<div
								class={`group flex items-center gap-1 rounded-md px-1.5 py-1.5 transition-colors ${isActive ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground'}`}
							>
								{#if hasChildren}
									<button
										class="flex size-4 shrink-0 items-center justify-center rounded hover:bg-muted"
										type="button"
										aria-label={isExpanded ? 'Collapse' : 'Expand'}
										aria-expanded={isExpanded}
										onclick={() => toggleExpanded(project.path)}
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
									{/if}
									{#if hasAttention}
										<CirclePauseIcon class="size-3 shrink-0 text-amber-400" />
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
										<DropdownMenu.Item onclick={() => projectStore.openProject(project.path)}>
											<ExternalLinkIcon class="size-3.5" />
											Open
										</DropdownMenu.Item>
										<DropdownMenu.Item
											onclick={() => claudeSessionStore.startSessionByProject(project.path)}
										>
											<SparklesIcon class="size-3.5" />
											New Claude Session
										</DropdownMenu.Item>
										<DropdownMenu.Item onclick={() => worktreeManager.add(project.path)}>
											<GitBranchIcon class="size-3.5" />
											Add Worktree
										</DropdownMenu.Item>
										<DropdownMenu.Item onclick={() => openInVSCode(project.path)}>
											<CodeIcon class="size-3.5" />
											Open in VS Code
										</DropdownMenu.Item>
										{#if tasks.length > 0}
											<DropdownMenu.Separator />
											<DropdownMenu.Group>
												<DropdownMenu.GroupHeading>Tasks</DropdownMenu.GroupHeading>
												{#each tasks as task, i (`${task.name}-${i}`)}
													<DropdownMenu.Item onclick={() => runTask(project, task)}>
														<PlayIcon class="size-3.5" />
														{task.name}
													</DropdownMenu.Item>
												{/each}
											</DropdownMenu.Group>
										{/if}
										<DropdownMenu.Separator />
										<DropdownMenu.Item onclick={() => projectManager.edit(project.path)}>
											<PencilIcon class="size-3.5" />
											Edit
										</DropdownMenu.Item>
										<DropdownMenu.Item
											class="text-destructive"
											onclick={() => projectManager.remove(project.path)}
										>
											<Trash2Icon class="size-3.5" />
											Remove
										</DropdownMenu.Item>
									</DropdownMenu.Content>
								</DropdownMenu.Root>
							</div>

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
											<ContextMenu.Root>
												<ContextMenu.Trigger class="w-full">
													<button
														class="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-muted-foreground transition-colors hover:bg-accent/50 hover:text-foreground"
														type="button"
														onclick={() => worktreeManager.open(project.path, wt.path, wt.branch)}
													>
														<GitBranchIcon class="size-3 shrink-0 text-emerald-400" />
														<span class="truncate text-xs font-medium">{wt.branch}</span>
													</button>
												</ContextMenu.Trigger>
												<ContextMenu.Content class="w-40">
													<ContextMenu.Item
														onclick={() => worktreeManager.open(project.path, wt.path, wt.branch)}
													>
														<ExternalLinkIcon class="size-3.5" />
														Open
													</ContextMenu.Item>
													<ContextMenu.Separator />
													<ContextMenu.Item
														class="text-destructive"
														onclick={() => worktreeManager.remove(project.path, wt.path)}
													>
														<Trash2Icon class="size-3.5" />
														Remove
													</ContextMenu.Item>
												</ContextMenu.Content>
											</ContextMenu.Root>
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
									{#each sessions as session (session.tabId)}
										<ContextMenu.Root>
											<ContextMenu.Trigger class="w-full">
												<button
													class="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-muted-foreground transition-colors hover:bg-accent/50 hover:text-foreground"
													type="button"
													onclick={() =>
														workspaceStore.selectTabByProject(project.path, session.tabId)}
												>
													{#if session.needsAttention}
														<CirclePauseIcon class="size-3 shrink-0 text-amber-400" />
													{:else}
														<LoaderCircleIcon
															class="size-3 shrink-0 animate-spin text-violet-400"
														/>
													{/if}
													<span
														class="truncate text-xs font-medium"
														class:text-amber-300={session.needsAttention}>{session.label}</span
													>
												</button>
											</ContextMenu.Trigger>
											<ContextMenu.Content class="w-40">
												<ContextMenu.Item
													onclick={() =>
														workspaceStore.restartClaudeByProject(project.path, session.tabId)}
												>
													<RotateCcwIcon class="size-3.5" />
													Restart
												</ContextMenu.Item>
												<ContextMenu.Separator />
												<ContextMenu.Item
													class="text-destructive"
													onclick={() =>
														workspaceStore.closeTabByProject(project.path, session.tabId)}
												>
													<XIcon class="size-3.5" />
													Close
												</ContextMenu.Item>
											</ContextMenu.Content>
										</ContextMenu.Root>
									{/each}
									{#if sessions.length > 0}
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
