import { invoke } from '@tauri-apps/api/core';
import type { ProjectConfig } from '$types/workbench';
import type { WorkspaceStore } from './workspaces.svelte';

export class ProjectStore {
	projects: ProjectConfig[] = $state([]);
	loaded = $state(false);
	private workspaces: WorkspaceStore;

	constructor(workspaces: WorkspaceStore) {
		this.workspaces = workspaces;
	}

	async load() {
		this.projects = await invoke<ProjectConfig[]>('list_projects');
		this.loaded = true;
	}

	async persist() {
		await invoke('save_projects', { projects: this.projects });
	}

	getByPath(projectPath: string): ProjectConfig | undefined {
		return this.projects.find((p) => p.path === projectPath);
	}

	async add(project: ProjectConfig) {
		this.projects = [...this.projects, project];
		await this.persist();
	}

	async update(previousPath: string, project: ProjectConfig) {
		this.projects = this.projects.map((p) => (p.path === previousPath ? project : p));
		await this.persist();
	}

	async remove(projectPath: string) {
		this.projects = this.projects.filter((p) => p.path !== projectPath);
		await this.persist();
	}

	reorder(fromPath: string, toPath: string) {
		const fromIndex = this.projects.findIndex((p) => p.path === fromPath);
		const toIndex = this.projects.findIndex((p) => p.path === toPath);
		if (fromIndex === -1 || toIndex === -1 || fromIndex === toIndex) return;

		const next = [...this.projects];
		const [moved] = next.splice(fromIndex, 1);
		next.splice(toIndex, 0, moved);
		this.projects = next;
		this.persist();
	}

	/** Open a project workspace (find by path, then open in workspace store) */
	openProject(projectPath: string) {
		const project = this.getByPath(projectPath);
		if (!project) return;
		this.workspaces.open(project);
	}

	/** Close all workspaces for a project, then remove it from the project list */
	async removeWithWorkspaces(projectPath: string) {
		this.workspaces.closeAllForProject(projectPath);
		await this.remove(projectPath);
	}
}
