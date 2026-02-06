<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import CodeIcon from '@lucide/svelte/icons/code';
	import EllipsisVerticalIcon from '@lucide/svelte/icons/ellipsis-vertical';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import PanelLeftCloseIcon from '@lucide/svelte/icons/panel-left-close';
	import PanelLeftOpenIcon from '@lucide/svelte/icons/panel-left-open';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import TerminalSquareIcon from '@lucide/svelte/icons/terminal-square';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { SvelteSet } from 'svelte/reactivity';
	import type { ActiveClaudeSession, ProjectConfig } from '$types/workbench';

	let {
		projects,
		loaded,
		sidebarCollapsed,
		activeProjectPath,
		openProjectPaths,
		activeSessionsByProject,
		onAddProject,
		onOpenProject,
		onEditProject,
		onRemoveProject,
		onOpenInVSCode,
		onAddClaude,
		onSelectTab,
		onRestartSession,
		onCloseSession,
		onOpenSettings,
		onToggleSidebar
	}: {
		projects: ProjectConfig[];
		loaded: boolean;
		sidebarCollapsed: boolean;
		activeProjectPath: string | null;
		openProjectPaths: string[];
		activeSessionsByProject: Record<string, ActiveClaudeSession[]>;
		onAddProject: () => void;
		onOpenProject: (path: string) => void;
		onEditProject: (path: string) => void;
		onRemoveProject: (path: string) => void;
		onOpenInVSCode: (path: string) => void;
		onAddClaude: (projectPath: string) => void;
		onSelectTab: (projectPath: string, tabId: string) => void;
		onRestartSession: (projectPath: string, tabId: string) => void;
		onCloseSession: (projectPath: string, tabId: string) => void;
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
		return activeSessionsByProject[projectPath] ?? [];
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
				onclick={onAddProject}
			>
				<PlusIcon class="size-3.5" />
				Add Project
			</Button>
		</div>

		<ScrollArea class="flex-1">
			<div class="space-y-0.5 px-2 pb-2">
				{#if !loaded}
					<p class="px-2 py-8 text-center text-xs text-muted-foreground">Loading...</p>
				{:else if projects.length === 0}
					<div class="px-2 py-8 text-center">
						<p class="text-xs text-muted-foreground">No projects yet.</p>
						<p class="mt-1 text-xs text-muted-foreground/60">Add a folder to get started.</p>
					</div>
				{:else}
					{#each projects as project (project.path)}
						{@const isOpen = openProjectPaths.includes(project.path)}
						{@const isActive = activeProjectPath === project.path}
						{@const sessions = sessionsForProject(project.path)}
						{@const isExpanded =
							expandedProjects.has(project.path) || project.path === activeProjectPath}
						<div>
							<div
								class={`group flex items-center gap-1 rounded-md px-1.5 py-1.5 transition-colors ${isActive ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground'}`}
							>
								{#if sessions.length > 0}
									<button
										class="flex size-4 shrink-0 items-center justify-center rounded hover:bg-muted"
										type="button"
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
									onclick={() => onOpenProject(project.path)}
								>
									<FolderIcon class="size-3.5 shrink-0 opacity-60" />
									<span class="truncate text-sm">{project.name}</span>
									{#if isOpen}
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
										<DropdownMenu.Item onclick={() => onOpenProject(project.path)}>
											<ExternalLinkIcon class="size-3.5" />
											Open
										</DropdownMenu.Item>
										<DropdownMenu.Item onclick={() => onAddClaude(project.path)}>
											<SparklesIcon class="size-3.5" />
											New Claude Session
										</DropdownMenu.Item>
										<DropdownMenu.Item onclick={() => onOpenInVSCode(project.path)}>
											<CodeIcon class="size-3.5" />
											Open in VS Code
										</DropdownMenu.Item>
										<DropdownMenu.Separator />
										<DropdownMenu.Item onclick={() => onEditProject(project.path)}>
											<PencilIcon class="size-3.5" />
											Edit
										</DropdownMenu.Item>
										<DropdownMenu.Item
											class="text-destructive"
											onclick={() => onRemoveProject(project.path)}
										>
											<Trash2Icon class="size-3.5" />
											Remove
										</DropdownMenu.Item>
									</DropdownMenu.Content>
								</DropdownMenu.Root>
							</div>

							{#if isExpanded && sessions.length > 0}
								<div class="mt-0.5 ml-5 space-y-0.5 border-l border-border/40 pl-2">
									{#each sessions as session (session.tabId)}
										<ContextMenu.Root>
											<ContextMenu.Trigger class="w-full">
												<button
													class="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-muted-foreground transition-colors hover:bg-accent/50 hover:text-foreground"
													type="button"
													onclick={() => onSelectTab(project.path, session.tabId)}
												>
													<SparklesIcon class="size-3 shrink-0 text-violet-400" />
													<span class="truncate text-xs font-medium">{session.label}</span>
												</button>
											</ContextMenu.Trigger>
											<ContextMenu.Content class="w-40">
												<ContextMenu.Item
													onclick={() => onRestartSession(project.path, session.tabId)}
												>
													<RotateCcwIcon class="size-3.5" />
													Restart
												</ContextMenu.Item>
												<ContextMenu.Separator />
												<ContextMenu.Item
													class="text-destructive"
													onclick={() => onCloseSession(project.path, session.tabId)}
												>
													<XIcon class="size-3.5" />
													Close
												</ContextMenu.Item>
											</ContextMenu.Content>
										</ContextMenu.Root>
									{/each}
									<button
										class="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-muted-foreground/60 transition-colors hover:bg-accent/50 hover:text-foreground"
										type="button"
										onclick={() => onAddClaude(project.path)}
									>
										<PlusIcon class="size-3 shrink-0" />
										<span class="text-xs">New Session</span>
									</button>
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
						onclick={onAddProject}
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
