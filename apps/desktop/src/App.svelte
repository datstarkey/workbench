<script lang="ts">
	import EmptyState from '$components/EmptyState.svelte';
	import ActivityRail from '$features/chrome/ActivityRail.svelte';
	import StatusBar from '$features/chrome/StatusBar.svelte';
	import IntegrationApprovalDialog from '$components/IntegrationApprovalDialog.svelte';
	import ProjectManager from '$features/projects/ProjectManager.svelte';
	import { ProjectManagerStore } from '$features/projects/project-manager.svelte';
	import ProjectSidebar from '$features/projects/ProjectSidebar.svelte';
	import RemoteInstanceSidebar from '$features/instances/RemoteInstanceSidebar.svelte';
	import ConnectInstanceDialog from '$features/instances/ConnectInstanceDialog.svelte';
	import { InstancesStore } from '$features/instances/instances.svelte';
	import NativeTerminalGrid from '$features/terminal/NativeTerminalGrid.svelte';
	import TerminalGrid from '$features/terminal/TerminalGrid.svelte';
	import TerminalTabs from '$features/terminal/TerminalTabs.svelte';
	import WorkspaceLanding from '$features/workspaces/WorkspaceLanding.svelte';
	import RightSidebar from '$features/sidebar/RightSidebar.svelte';
	import WorkspaceTabs from '$features/workspaces/WorkspaceTabs.svelte';
	import WorktreeManager from '$features/worktrees/WorktreeManager.svelte';
	import { WorktreeManagerStore } from '$features/worktrees/worktree-manager.svelte';
	import * as Resizable from '@workbench/ui/resizable';
	import * as Tooltip from '@workbench/ui/tooltip';
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
		setInstancesStore,
		setProjectManager,
		setProjectStore,
		setSidebarStore,
		setTrelloStore,
		setUpdaterStore,
		setWorkbenchSettingsStore,
		setWorktreeManager,
		setWorkspaceStore
	} from '$stores/context';
	import { GitHubStore } from '$stores/github.svelte';
	import { NotificationStore } from '$stores/notifications.svelte';
	import { TrelloStore } from '$stores/trello.svelte';
	import { UpdaterStore } from '$stores/updater.svelte';
	import { GitStore } from '$stores/git.svelte';
	import { SidebarStore } from '$stores/sidebar.svelte';
	import { openSettingsWindow } from '$lib/utils/settings-window';
	import { WorkbenchSettingsStore } from '$stores/workbench-settings.svelte';
	import { ProjectStore } from '$stores/projects.svelte';
	import { WorkspaceStore } from '$stores/workspaces.svelte';
	import { listen } from '@tauri-apps/api/event';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { startServer } from '$lib/server-mode';
	import { onMount } from 'svelte';
	import { watch } from 'runed';
	import { Toaster, toast } from 'svelte-sonner';

	const workbenchSettingsStore = setWorkbenchSettingsStore(new WorkbenchSettingsStore());
	const gitStore = setGitStore(new GitStore());
	const workspaceStore = setWorkspaceStore(new WorkspaceStore());
	const projectStore = setProjectStore(new ProjectStore(workspaceStore));
	const integrationApprovalStore = setIntegrationApprovalStore(new IntegrationApprovalStore());
	const claudeSessionStore = setClaudeSessionStore(
		new ClaudeSessionStore(workspaceStore, projectStore, integrationApprovalStore)
	);
	const githubStore = setGitHubStore(new GitHubStore());
	setClaudeSettingsStore(new ClaudeSettingsStore());
	setUpdaterStore(new UpdaterStore());
	setProjectManager(new ProjectManagerStore(projectStore, workspaceStore, gitStore));
	setWorktreeManager(
		new WorktreeManagerStore(
			projectStore,
			workspaceStore,
			gitStore,
			githubStore,
			workbenchSettingsStore
		)
	);
	const trelloStore = setTrelloStore(new TrelloStore());
	setSidebarStore(new SidebarStore());
	const instancesStore = setInstancesStore(new InstancesStore());
	new NotificationStore(workspaceStore, claudeSessionStore);

	let sidebarCollapsed = $state(false);
	let sidebarPane = $state<ReturnType<typeof Resizable.Pane> | null>(null);
	let githubSidebarPane = $state<ReturnType<typeof Resizable.Pane> | null>(null);
	let connectOpen = $state(false);

	/** Open (or focus) settings in its own OS window, seeded with the active project. */
	function openSettings() {
		void openSettingsWindow(
			workspaceStore.activeProjectPath,
			workbenchSettingsStore.settingsWindowBounds
		);
	}

	function toggleSidebar() {
		if (sidebarCollapsed) {
			sidebarPane?.expand();
		} else {
			sidebarPane?.collapse();
		}
	}

	listen('menu:open-settings', () => {
		openSettings();
	});

	// The settings window persists changes to disk in its own webview; reload the
	// live-bound stores here so accent, sidebar toggles, and integrations update.
	listen('settings:changed', async () => {
		await workbenchSettingsStore.load();
		await trelloStore.loadCredentials();
		await Promise.all(projectStore.projects.map((p) => trelloStore.loadProjectConfig(p.path)));
	});

	// Apply selected accent preset to the document root (drives CSS-var presets).
	// watch() tracks only accentColor and runs the DOM write untracked.
	watch(
		() => workbenchSettingsStore.accentColor,
		(accent) => document.documentElement.setAttribute('data-accent', accent)
	);

	// Update macOS dock badge with count of sessions awaiting user input
	watch(
		() => claudeSessionStore.panesAwaitingInput.size,
		(count) => {
			getCurrentWindow().setBadgeCount(count > 0 ? count : undefined);
		}
	);

	// Sync tracked projects for backend GitHub polling
	watch(
		() => [githubStore.ghAvailable, githubStore.trackedProjectPaths],
		() => {
			void githubStore.syncTrackedProjects();
		}
	);

	// Refresh git/GitHub data when switching workspaces (throttled to 2s)
	let lastSwitchRefresh = 0;
	workspaceStore.onWorkspaceSwitch((projectPath) => {
		const now = Date.now();
		if (now - lastSwitchRefresh < 2000) return;
		lastSwitchRefresh = now;
		void gitStore.refreshGitState(projectPath);
		void githubStore.refreshProject(projectPath);
	});

	// Sync githubStore.sidebarOpen → pane expand/collapse (imperative DOM API)
	watch(
		() => githubStore.sidebarOpen,
		(open) => {
			if (open) {
				githubSidebarPane?.expand();
			} else {
				githubSidebarPane?.collapse();
			}
		}
	);

	// Register check completion toast handler (non-reactive callback)
	githubStore.onCheckComplete((n) => {
		if (n.bucket === 'pass') {
			toast.success(`${n.name} passed`);
		} else {
			toast.error(`${n.name} failed`);
		}
	});

	onMount(async () => {
		instancesStore.load();
		await Promise.all([workbenchSettingsStore.load(), projectStore.load()]);
		await workspaceStore.load();
		workspaceStore.ensureShape();
		if (workspaceStore.workspaces.length === 0 && projectStore.projects.length === 1) {
			projectStore.openProject(projectStore.projects[0].path);
		}
		gitStore.refreshAll(projectStore.projects.map((p) => p.path));
		githubStore.initForProjects(projectStore.projects.map((p) => p.path));
		githubStore.initSidebarState();
		await trelloStore.loadCredentials();
		await Promise.all(projectStore.projects.map((p) => trelloStore.loadProjectConfig(p.path)));
		if (workbenchSettingsStore.serverMode) {
			try {
				await startServer(workbenchSettingsStore.serverPort);
			} catch {
				/* server failed to start (e.g. port in use); surfaced in settings */
			}
		}
	});
</script>

<Tooltip.Provider>
	<div class="flex h-screen flex-col overflow-hidden bg-background text-foreground">
		<div class="flex min-h-0 flex-1">
			<ActivityRail
				onToggleSidebar={toggleSidebar}
				onOpenSettings={openSettings}
				onOpenRemote={() => (connectOpen = true)}
			/>
			<div class="h-full min-w-0 flex-1">
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
						{#if instancesStore.activeIsLocal || !instancesStore.activeRemote}
							<ProjectSidebar
								{sidebarCollapsed}
								onOpenSettings={openSettings}
								onToggleSidebar={toggleSidebar}
								onConnect={() => (connectOpen = true)}
							/>
						{:else}
							<RemoteInstanceSidebar
								instance={instancesStore.activeRemote}
								{sidebarCollapsed}
								onToggleSidebar={toggleSidebar}
								onConnect={() => (connectOpen = true)}
							/>
						{/if}
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
										ws.terminalTabs.find((t) => t.id === ws.activeTerminalTabId) ??
										ws.terminalTabs[0]}
									{@const wsProject = projectStore.getByPath(ws.projectPath)}
									<div class="flex min-h-0 flex-1 flex-col" class:hidden={!isActiveWs}>
										{#if activeTab && wsProject}
											<TerminalTabs workspace={ws} />

											<div class="relative min-h-0 flex-1">
												{#each ws.terminalTabs as tab (tab.id)}
													{@const isActiveTab = tab.id === ws.activeTerminalTabId}
													<div
														class="absolute inset-0 flex"
														class:invisible={!isActiveTab || !isActiveWs}
														class:z-10={isActiveTab && isActiveWs}
														class:z-0={!isActiveTab || !isActiveWs}
													>
														{#if ws.renderer === 'native'}
															<NativeTerminalGrid
																panes={tab.panes}
																active={isActiveTab && isActiveWs}
																project={wsProject}
																cwd={ws.worktreePath}
															/>
														{:else}
															<TerminalGrid
																workspaceId={ws.id}
																panes={tab.panes}
																split={tab.split}
																active={isActiveTab && isActiveWs}
																project={wsProject}
																cwd={ws.worktreePath}
															/>
														{/if}
													</div>
												{/each}
											</div>
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
							<RightSidebar onClose={() => githubStore.toggleSidebar()} />
						</Resizable.Pane>
					{/if}
				</Resizable.PaneGroup>
			</div>
		</div>
		<StatusBar />
	</div>
</Tooltip.Provider>

<ProjectManager />
<WorktreeManager />
<ConnectInstanceDialog bind:open={connectOpen} />
<IntegrationApprovalDialog />
<UpdateDialog />
<Toaster theme="dark" position="bottom-right" />
