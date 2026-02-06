<script lang="ts">
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import * as Resizable from '$lib/components/ui/resizable';
	import { Button } from '$lib/components/ui/button';
	import ProjectSidebar from '$components/ProjectSidebar.svelte';
	import WorkspaceTabs from '$components/WorkspaceTabs.svelte';
	import TerminalTabs from '$components/TerminalTabs.svelte';
	import TerminalGrid from '$components/TerminalGrid.svelte';
	import ProjectDialog from '$components/ProjectDialog.svelte';
	import EmptyState from '$components/EmptyState.svelte';
	import ConfirmDialog from '$components/ConfirmDialog.svelte';
	import SettingsSheet from '$components/settings/SettingsSheet.svelte';
	import { projectStore } from '$stores/projects.svelte';
	import { workspaceStore } from '$stores/workspaces.svelte';
	import { claudeSessionStore } from '$stores/claudeSessions.svelte';
	import { selectFolder } from '$lib/hooks/useDialog.svelte';
	import type {
		ActiveClaudeSession,
		DiscoveredClaudeSession,
		ProjectConfig,
		ProjectFormState
	} from '$types/workbench';

	let sidebarCollapsed = $state(false);
	let discoveredSessions: DiscoveredClaudeSession[] = $state([]);
	let sidebarPane = $state<ReturnType<typeof Resizable.Pane> | null>(null);

	function toggleSidebar() {
		if (sidebarCollapsed) {
			sidebarPane?.expand();
		} else {
			sidebarPane?.collapse();
		}
	}
	let projectDialogOpen = $state(false);
	let projectDialogMode: 'create' | 'edit' = $state('create');
	let editingProjectPath: string | null = $state(null);
	let projectForm: ProjectFormState = $state({ name: '', path: '', shell: '', startupCommand: '' });
	let projectFormError = $state('');
	let confirmDialogOpen = $state(false);
	let pendingRemovePath: string | null = $state(null);
	let settingsOpen = $state(false);

	let openProjectPaths = $derived(workspaceStore.workspaces.map((w) => w.projectPath));
	let activeProjectPath = $derived(workspaceStore.activeWorkspace?.projectPath ?? null);

	let activeSessionsByProject = $derived(
		workspaceStore.workspaces.reduce<Record<string, ActiveClaudeSession[]>>((acc, ws) => {
			const sessions = ws.terminalTabs
				.filter((t) => t.type === 'claude')
				.map((t) => ({
					claudeSessionId: t.panes[0]?.claudeSessionId ?? '',
					tabId: t.id,
					label: t.label
				}));
			if (sessions.length > 0) {
				acc[ws.projectPath] = sessions;
			}
			return acc;
		}, {})
	);

	const baseName = (p: string) => {
		const segments = p.replace(/\\/g, '/').split('/').filter(Boolean);
		return segments[segments.length - 1] || p;
	};

	function resetProjectForm() {
		projectForm = { name: '', path: '', shell: '', startupCommand: '' };
		projectFormError = '';
		editingProjectPath = null;
	}

	async function openCreateProjectDialog() {
		projectDialogMode = 'create';
		resetProjectForm();
		projectDialogOpen = true;
		projectFormError = 'Choose a project folder to continue.';

		const selectedPath = await selectFolder();
		if (!selectedPath) return;
		projectForm = { ...projectForm, path: selectedPath, name: baseName(selectedPath) };
		projectFormError = '';
	}

	function openEditProjectDialog(projectPath: string) {
		const project = projectStore.getByPath(projectPath);
		if (!project) return;
		projectDialogMode = 'edit';
		editingProjectPath = project.path;
		projectForm = {
			name: project.name,
			path: project.path,
			shell: project.shell || '',
			startupCommand: project.startupCommand || ''
		};
		projectFormError = '';
		projectDialogOpen = true;
	}

	async function pickProjectFolder() {
		const selectedPath = await selectFolder(projectForm.path.trim() || undefined);
		if (!selectedPath) return;
		projectForm = { ...projectForm, path: selectedPath };
		if (!projectForm.name.trim()) {
			projectForm = { ...projectForm, name: baseName(selectedPath) };
		}
	}

	async function saveProjectForm() {
		const nextName = projectForm.name.trim();
		const nextPath = projectForm.path.trim();
		if (!nextName) {
			projectFormError = 'Project name is required.';
			return;
		}
		if (!nextPath) {
			projectFormError = 'Select a project folder.';
			return;
		}

		const duplicate = projectStore.projects.find((p) => p.path === nextPath);
		if (duplicate && duplicate.path !== editingProjectPath) {
			projectFormError = 'That folder is already added as a project.';
			return;
		}

		const nextProject: ProjectConfig = {
			name: nextName,
			path: nextPath,
			shell: projectForm.shell.trim() || undefined,
			startupCommand: projectForm.startupCommand.trim() || undefined
		};

		if (projectDialogMode === 'create') {
			await projectStore.add(nextProject);
			openProject(nextProject.path);
		} else {
			const previousPath = editingProjectPath;
			if (!previousPath) {
				projectFormError = 'Could not determine which project to update.';
				return;
			}
			await projectStore.update(previousPath, nextProject);
			workspaceStore.updateProjectInfo(previousPath, nextProject.path, nextProject.name);
		}

		projectDialogOpen = false;
		resetProjectForm();
	}

	function removeProject(projectPath: string) {
		pendingRemovePath = projectPath;
		confirmDialogOpen = true;
	}

	async function confirmRemoveProject() {
		if (!pendingRemovePath) return;
		const ws = workspaceStore.getByProjectPath(pendingRemovePath);
		if (ws) workspaceStore.close(ws.id);
		await projectStore.remove(pendingRemovePath);
		pendingRemovePath = null;
	}

	function registerClaudeSession(claudeSessionId: string, tabLabel: string, projectPath: string) {
		claudeSessionStore.addSession({
			id: claudeSessionId,
			label: tabLabel,
			projectPath,
			createdAt: new Date().toISOString()
		});
	}

	function openProject(projectPath: string) {
		const project = projectStore.getByPath(projectPath);
		if (!project) return;
		const result = workspaceStore.open(project);
		if (result) {
			registerClaudeSession(result.claudeSessionId, result.tabLabel, project.path);
		}
	}

	async function openInVSCode(projectPath: string) {
		try {
			await invoke('open_in_vscode', { path: projectPath });
		} catch {
			// VS Code launch failed silently
		}
	}

	function handleSidebarSelectTab(projectPath: string, tabId: string) {
		const ws = workspaceStore.getByProjectPath(projectPath);
		if (ws) {
			workspaceStore.selectedId = ws.id;
			workspaceStore.setActiveTab(ws.id, tabId);
		}
	}

	function handleSidebarAddClaude(projectPath: string) {
		openProject(projectPath);
		const ws = workspaceStore.getByProjectPath(projectPath);
		if (!ws) return;
		const { tabLabel, claudeSessionId } = workspaceStore.addClaudeSession(ws.id);
		registerClaudeSession(claudeSessionId, tabLabel, projectPath);
	}

	$effect(() => {
		workspaceStore.ensureShape((path) => projectStore.getByPath(path));
	});

	onMount(async () => {
		await projectStore.load();
		await workspaceStore.load();
		await claudeSessionStore.load();
		if (workspaceStore.workspaces.length === 0 && projectStore.projects.length === 1) {
			openProject(projectStore.projects[0].path);
		}
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
					projects={projectStore.projects}
					loaded={projectStore.loaded}
					{sidebarCollapsed}
					{activeProjectPath}
					{openProjectPaths}
					{activeSessionsByProject}
					onAddProject={openCreateProjectDialog}
					onOpenProject={openProject}
					onEditProject={openEditProjectDialog}
					onRemoveProject={removeProject}
					onOpenInVSCode={openInVSCode}
					onAddClaude={handleSidebarAddClaude}
					onSelectTab={handleSidebarSelectTab}
					onOpenSettings={() => (settingsOpen = true)}
					onToggleSidebar={toggleSidebar}
				/>
			</Resizable.Pane>
			<Resizable.Handle withHandle class="cursor-col-resize" />
			<Resizable.Pane defaultSize={83} minSize={50} class="h-full">
				<main class="flex h-full min-w-0 flex-1 flex-col">
					{#if workspaceStore.workspaces.length === 0}
						<EmptyState onAddProject={openCreateProjectDialog} />
					{:else}
						<WorkspaceTabs
							workspaces={workspaceStore.workspaces}
							activeWorkspaceId={workspaceStore.activeWorkspaceId}
							onSelect={(id) => (workspaceStore.selectedId = id)}
							onClose={(id) => workspaceStore.close(id)}
							onReorder={(from, to) => workspaceStore.reorder(from, to)}
							onOpenInVSCode={openInVSCode}
						/>

						{#each workspaceStore.workspaces as ws (ws.id)}
							{@const isActiveWs = ws.id === workspaceStore.activeWorkspaceId}
							{@const activeTab =
								ws.terminalTabs.find((t) => t.id === ws.activeTerminalTabId) ?? ws.terminalTabs[0]}
							{@const wsProject = projectStore.getByPath(ws.projectPath)}
							<div class="flex min-h-0 flex-1 flex-col" class:hidden={!isActiveWs}>
								{#if activeTab}
									<TerminalTabs
										tabs={ws.terminalTabs}
										activeTabId={ws.activeTerminalTabId}
										onSelect={(id) => workspaceStore.setActiveTab(ws.id, id)}
										onClose={(id) => workspaceStore.closeTerminalTab(ws.id, id)}
										onAdd={() => {
											if (wsProject) workspaceStore.addTerminalTab(ws.id, wsProject);
										}}
										onAddClaude={() => {
											const { tabLabel, claudeSessionId } = workspaceStore.addClaudeSession(ws.id);
											registerClaudeSession(claudeSessionId, tabLabel, ws.projectPath);
										}}
										onResumeClaude={(sessionId, label) =>
											workspaceStore.resumeClaudeSession(ws.id, sessionId, label)}
										{discoveredSessions}
										onDiscoverSessions={async () => {
											discoveredSessions = await claudeSessionStore.discoverSessions(
												ws.projectPath
											);
										}}
										onSplitHorizontal={() => workspaceStore.splitTerminal(ws.id, 'horizontal')}
										onSplitVertical={() => workspaceStore.splitTerminal(ws.id, 'vertical')}
									/>

									{#if wsProject}
										{#each ws.terminalTabs as tab (tab.id)}
											{@const isActiveTab = tab.id === ws.activeTerminalTabId}
											<div
												class="min-h-0 flex-1"
												class:hidden={!isActiveTab || !isActiveWs}
												class:flex={isActiveTab && isActiveWs}
											>
												<TerminalGrid
													panes={tab.panes}
													split={tab.split}
													active={isActiveTab && isActiveWs}
													project={wsProject}
													onRemovePane={(paneId) => workspaceStore.removePane(ws.id, paneId)}
												/>
											</div>
										{/each}
									{/if}
								{:else}
									<div class="flex flex-1 items-center justify-center">
										<div class="text-center">
											<h2 class="text-lg font-semibold tracking-tight">Workspace needs repair</h2>
											<p class="mt-1 text-sm text-muted-foreground">
												Terminal state is inconsistent.
											</p>
											<Button
												type="button"
												class="mt-4"
												onclick={() => workspaceStore.ensureShape((p) => projectStore.getByPath(p))}
											>
												Repair Workspace
											</Button>
										</div>
									</div>
								{/if}
							</div>
						{/each}
					{/if}
				</main>
			</Resizable.Pane>
		</Resizable.PaneGroup>
	</div>
</Tooltip.Provider>

<ProjectDialog
	bind:open={projectDialogOpen}
	mode={projectDialogMode}
	bind:form={projectForm}
	error={projectFormError}
	onSave={saveProjectForm}
	onPickFolder={pickProjectFolder}
/>

<ConfirmDialog
	bind:open={confirmDialogOpen}
	title="Remove Project"
	description="Remove this project from Workbench? This won't delete the folder."
	confirmLabel="Remove"
	destructive
	onConfirm={confirmRemoveProject}
/>

<SettingsSheet bind:open={settingsOpen} projectPath={activeProjectPath} />
