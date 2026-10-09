import { groupProjects } from '@workbench/control-plane-ui';
import { SvelteSet } from 'svelte/reactivity';
import { invoke } from '$lib/transport';
import type { ProjectConfig } from '$types/workbench';
import type { WorkspaceStore } from './workspaces.svelte';

const FAVOURITES_KEY = 'workbench.favourites';

function readFavourites(): string[] {
	try {
		const parsed: unknown = JSON.parse(localStorage.getItem(FAVOURITES_KEY) ?? '[]');
		return Array.isArray(parsed) ? parsed.filter((v) => typeof v === 'string') : [];
	} catch {
		return [];
	}
}

export class ProjectStore {
	projects: ProjectConfig[] = $state([]);
	loaded = $state(false);
	/** Favourite project paths, pinned to the top of the sidebar; remembered per machine. */
	readonly favourites = new SvelteSet<string>(readFavourites());
	/** Sidebar section keys (`projectSections`) the user collapsed; this session only. */
	readonly collapsedSections = new SvelteSet<string>();
	private workspaces: WorkspaceStore;

	/** Unique group names in first-appearance order */
	groupNames: string[] = $derived(
		groupProjects(this.projects)
			.map((g) => g.group)
			.filter((g): g is string => g !== null)
	);

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
		if (previousPath !== project.path && this.favourites.delete(previousPath)) {
			this.favourites.add(project.path);
			this.saveFavourites();
		}
		const before = this.getByPath(previousPath);
		this.projects = this.projects.map((p) => (p.path === previousPath ? project : p));
		await this.persist();
		// Open workspaces follow a moved or renamed project, on every device.
		if (before && (before.path !== project.path || before.name !== project.name))
			await this.workspaces.updateProject(previousPath, project);
	}

	async remove(projectPath: string) {
		if (this.favourites.delete(projectPath)) this.saveFavourites();
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

	toggleFavourite(projectPath: string) {
		if (this.favourites.delete(projectPath)) {
			this.saveFavourites();
			return;
		}
		this.favourites.add(projectPath);
		// A new favourite must stay visible, so a collapsed Favourites reopens.
		this.collapsedSections.delete('favourites');
		this.saveFavourites();
	}

	toggleSection(key: string) {
		if (!this.collapsedSections.delete(key)) this.collapsedSections.add(key);
	}

	private saveFavourites() {
		try {
			localStorage.setItem(FAVOURITES_KEY, JSON.stringify([...this.favourites]));
		} catch {
			// Blocked storage only loses the preference.
		}
	}

	/** Set or clear the group for a project */
	async setGroup(projectPath: string, group: string | undefined) {
		this.projects = this.projects.map((p) =>
			p.path === projectPath ? { ...p, group: group || undefined } : p
		);
		await this.persist();
	}

	/** Open a project workspace (find by path, then open in workspace store) */
	openProject(projectPath: string): Promise<unknown> {
		const project = this.getByPath(projectPath);
		return project ? this.workspaces.open(project) : Promise.resolve();
	}

	/** Close all workspaces for a project, then remove it from the project list */
	async removeWithWorkspaces(projectPath: string) {
		this.workspaces.closeAllForProject(projectPath);
		await this.remove(projectPath);
	}
}
