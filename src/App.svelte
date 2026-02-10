<script lang="ts">
	import EmptyState from '$components/EmptyState.svelte';
	import SettingsSheet from '$components/settings/SettingsSheet.svelte';
	import ProjectManager from '$features/projects/ProjectManager.svelte';
	import { ProjectManagerStore } from '$features/projects/project-manager.svelte';
	import ProjectSidebar from '$features/projects/ProjectSidebar.svelte';
	import TerminalGrid from '$features/terminal/TerminalGrid.svelte';
	import TerminalTabs from '$features/terminal/TerminalTabs.svelte';
	import WorkspaceLanding from '$features/workspaces/WorkspaceLanding.svelte';
	import WorkspaceTabs from '$features/workspaces/WorkspaceTabs.svelte';
	import WorktreeManager from '$features/worktrees/WorktreeManager.svelte';
	import { WorktreeManagerStore } from '$features/worktrees/worktree-manager.svelte';
	import * as Resizable from '$lib/components/ui/resizable';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { ClaudeSettingsStore } from '$stores/claude-settings.svelte';
	import { ClaudeSessionStore } from '$stores/claudeSessions.svelte';
	import {
		setClaudeSessionStore,
		setClaudeSettingsStore,
		setGitStore,
		setProjectManager,
		setProjectStore,
		setWorktreeManager,
		setWorkspaceStore
	} from '$stores/context';
	import { GitStore } from '$stores/git.svelte';
	import { ProjectStore } from '$stores/projects.svelte';
	import { WorkspaceStore } from '$stores/workspaces.svelte';
	import { onMount, untrack } from 'svelte';
	import { SvelteSet } from 'svelte/reactivity';

	const workspaceStore = setWorkspaceStore(new WorkspaceStore());
	const projectStore = setProjectStore(new ProjectStore(workspaceStore));
	setClaudeSessionStore(new ClaudeSessionStore(workspaceStore, projectStore));
	const gitStore = setGitStore(new GitStore());
	setClaudeSettingsStore(new ClaudeSettingsStore());
	setProjectManager(new ProjectManagerStore(projectStore, workspaceStore, gitStore));
	setWorktreeManager(new WorktreeManagerStore(projectStore, workspaceStore, gitStore));

	let sidebarCollapsed = $state(false);
	let sidebarPane = $state<ReturnType<typeof Resizable.Pane> | null>(null);
	let settingsOpen = $state(false);

	// Watch/unwatch git filesystem changes for open projects only
	let watchedPaths = new SvelteSet<string>();
	$effect(() => {
		const openPaths = new Set(workspaceStore.openProjectPaths);
		untrack(() => {
			// Watch newly opened projects
			for (const path of openPaths) {
				if (!watchedPaths.has(path)) {
					gitStore.watchProject(path);
					watchedPaths.add(path);
				}
			}
			// Unwatch closed projects
			for (const path of watchedPaths) {
				if (!openPaths.has(path)) {
					gitStore.unwatchProject(path);
					watchedPaths.delete(path);
				}
			}
		});
	});

	function toggleSidebar() {
		if (sidebarCollapsed) {
			sidebarPane?.expand();
		} else {
			sidebarPane?.collapse();
		}
	}

	onMount(async () => {
		await projectStore.load();
		await workspaceStore.load();
		workspaceStore.ensureShape();
		if (workspaceStore.workspaces.length === 0 && projectStore.projects.length === 1) {
			projectStore.openProject(projectStore.projects[0].path);
		}
		gitStore.refreshAll(projectStore.projects.map((p) => p.path));
	});
</script>

<Tooltip.Provider>
	<div class="h-screen overflow-hidden bg-background text-foreground">
		<Resizable.PaneGroup direction="horizontal">
			<Resizable.Pane
				bind:this={sidebarPane}
				defaultSize={17}
				minSize={12}
				maxSize={30}
				collapsible
				collapsedSize={3}
				onCollapse={() => (sidebarCollapsed = true)}
				onExpand={() => (sidebarCollapsed = false)}
				class="h-full"
			>
				<ProjectSidebar
					{sidebarCollapsed}
					onOpenSettings={() => (settingsOpen = true)}
					onToggleSidebar={toggleSidebar}
				/>
			</Resizable.Pane>
			<Resizable.Handle withHandle class="cursor-col-resize" />
			<Resizable.Pane defaultSize={83} minSize={50} class="h-full">
				<main class="flex h-full min-w-0 flex-1 flex-col">
					{#if workspaceStore.workspaces.length === 0}
						<EmptyState />
					{:else}
						<WorkspaceTabs />

						{#each workspaceStore.workspaces as ws (ws.id)}
							{@const isActiveWs = ws.id === workspaceStore.activeWorkspaceId}
							{@const activeTab =
								ws.terminalTabs.find((t) => t.id === ws.activeTerminalTabId) ?? ws.terminalTabs[0]}
							{@const wsProject = projectStore.getByPath(ws.projectPath)}
							<div class="flex min-h-0 flex-1 flex-col" class:hidden={!isActiveWs}>
								{#if activeTab && wsProject}
									<TerminalTabs workspace={ws} />

									{#each ws.terminalTabs as tab (tab.id)}
										{@const isActiveTab = tab.id === ws.activeTerminalTabId}
										<div
											class="min-h-0 flex-1"
											class:hidden={!isActiveTab || !isActiveWs}
											class:flex={isActiveTab && isActiveWs}
										>
											<TerminalGrid
												workspaceId={ws.id}
												panes={tab.panes}
												split={tab.split}
												active={isActiveTab && isActiveWs}
												project={wsProject}
												cwd={ws.worktreePath}
											/>
										</div>
									{/each}
								{:else}
									<WorkspaceLanding workspace={ws} />
								{/if}
							</div>
						{/each}
					{/if}
				</main>
			</Resizable.Pane>
		</Resizable.PaneGroup>
	</div>
</Tooltip.Provider>

<ProjectManager />
<WorktreeManager />
<SettingsSheet bind:open={settingsOpen} projectPath={workspaceStore.activeProjectPath} />
