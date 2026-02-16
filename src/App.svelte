<script lang="ts">
	import EmptyState from '$components/EmptyState.svelte';
	import IntegrationApprovalDialog from '$components/IntegrationApprovalDialog.svelte';
	import SettingsSheet from '$components/settings/SettingsSheet.svelte';
	import ProjectManager from '$features/projects/ProjectManager.svelte';
	import { ProjectManagerStore } from '$features/projects/project-manager.svelte';
	import ProjectSidebar from '$features/projects/ProjectSidebar.svelte';
	import TerminalGrid from '$features/terminal/TerminalGrid.svelte';
	import TerminalTabs from '$features/terminal/TerminalTabs.svelte';
	import WorkspaceLanding from '$features/workspaces/WorkspaceLanding.svelte';
	import GitHubSidebar from '$features/github/GitHubSidebar.svelte';
	import WorkspaceTabs from '$features/workspaces/WorkspaceTabs.svelte';
	import WorktreeManager from '$features/worktrees/WorktreeManager.svelte';
	import { WorktreeManagerStore } from '$features/worktrees/worktree-manager.svelte';
	import * as Resizable from '$lib/components/ui/resizable';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import UpdateDialog from '$components/UpdateDialog.svelte';
	import { ClaudeSettingsStore } from '$stores/claude-settings.svelte';
	import { ClaudeSessionStore } from '$stores/claudeSessions.svelte';
	import { IntegrationApprovalStore } from '$stores/integration-approval.svelte';
	import {
		setClaudeSessionStore,
		setClaudeSettingsStore,
		setGitHubStore,
		setGitStore,
		setIntegrationApprovalStore,
		setProjectManager,
		setProjectStore,
		setUpdaterStore,
		setWorkbenchSettingsStore,
		setWorktreeManager,
		setWorkspaceStore
	} from '$stores/context';
	import { GitHubStore } from '$stores/github.svelte';
	import { UpdaterStore } from '$stores/updater.svelte';
	import { GitStore } from '$stores/git.svelte';
	import { WorkbenchSettingsStore } from '$stores/workbench-settings.svelte';
	import { ProjectStore } from '$stores/projects.svelte';
	import { WorkspaceStore } from '$stores/workspaces.svelte';
	import { listen } from '@tauri-apps/api/event';
	import { onMount, untrack } from 'svelte';
	import { SvelteSet } from 'svelte/reactivity';

	const workspaceStore = setWorkspaceStore(new WorkspaceStore());
	const projectStore = setProjectStore(new ProjectStore(workspaceStore));
	const workbenchSettingsStore = setWorkbenchSettingsStore(new WorkbenchSettingsStore());
	const integrationApprovalStore = setIntegrationApprovalStore(new IntegrationApprovalStore());
	setClaudeSessionStore(
		new ClaudeSessionStore(workspaceStore, projectStore, integrationApprovalStore)
	);
	const gitStore = setGitStore(new GitStore());
	const githubStore = setGitHubStore(new GitHubStore());
	setClaudeSettingsStore(new ClaudeSettingsStore());
	setUpdaterStore(new UpdaterStore());
	setProjectManager(new ProjectManagerStore(projectStore, workspaceStore, gitStore));
	setWorktreeManager(
		new WorktreeManagerStore(projectStore, workspaceStore, gitStore, workbenchSettingsStore)
	);

	let sidebarCollapsed = $state(false);
	let sidebarPane = $state<ReturnType<typeof Resizable.Pane> | null>(null);
	let githubSidebarPane = $state<ReturnType<typeof Resizable.Pane> | null>(null);
	let settingsOpen = $state(false);

	function toggleSidebar() {
		if (sidebarCollapsed) {
			sidebarPane?.expand();
		} else {
			sidebarPane?.collapse();
		}
	}

	listen('menu:open-settings', () => {
		settingsOpen = true;
	});

	// Fetch GitHub status when projects gain active sessions (network side effect)
	$effect(() => {
		const branches = githubStore.activeBranches;
		untrack(() => {
			const seen = new SvelteSet<string>();
			for (const { projectPath } of branches) {
				if (!seen.has(projectPath)) {
					seen.add(projectPath);
					githubStore.fetchProjectStatus(projectPath);
				}
			}
		});
	});

	// Sync githubStore.sidebarOpen → pane expand/collapse (imperative DOM API)
	$effect(() => {
		const open = githubStore.sidebarOpen;
		untrack(() => {
			if (open) {
				githubSidebarPane?.expand();
			} else {
				githubSidebarPane?.collapse();
			}
		});
	});

	onMount(async () => {
		await workbenchSettingsStore.load();
		await projectStore.load();
		await workspaceStore.load();
		workspaceStore.ensureShape();
		if (workspaceStore.workspaces.length === 0 && projectStore.projects.length === 1) {
			projectStore.openProject(projectStore.projects[0].path);
		}
		gitStore.refreshAll(projectStore.projects.map((p) => p.path));
		githubStore.initForProjects(projectStore.projects.map((p) => p.path));
		githubStore.initSidebarState();
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
			{#if workspaceStore.workspaces.length > 0}
				<Resizable.Handle withHandle class="cursor-col-resize" />
				<Resizable.Pane
					bind:this={githubSidebarPane}
					defaultSize={0}
					minSize={15}
					maxSize={35}
					collapsible
					collapsedSize={0}
					onCollapse={() => (githubStore.sidebarOpen = false)}
					onExpand={() => (githubStore.sidebarOpen = true)}
					class="h-full"
				>
					<GitHubSidebar onClose={() => githubStore.toggleSidebar()} />
				</Resizable.Pane>
			{/if}
		</Resizable.PaneGroup>
	</div>
</Tooltip.Provider>

<ProjectManager />
<WorktreeManager />
<SettingsSheet bind:open={settingsOpen} projectPath={workspaceStore.activeProjectPath} />
<IntegrationApprovalDialog />
<UpdateDialog />
