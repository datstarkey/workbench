import { invoke } from '@tauri-apps/api/core';
import type { ProjectConfig } from '$types/workbench';

class ProjectStore {
	projects: ProjectConfig[] = $state([]);
	loaded = $state(false);

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
}

export const projectStore = new ProjectStore();
